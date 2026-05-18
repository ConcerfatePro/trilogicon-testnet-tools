//! HTTP routes for the faucet API.

use crate::config::Config;
use crate::db;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

const MAX_ADDRESS_LEN: usize = 256;
/// Small cap for `{"address":"..."}`; real validation stays minimal for this MVP.
const MAX_CLAIM_BODY_BYTES: usize = 512;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: SqlitePool,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub service: &'static str,
    pub dry_run: bool,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub ok: bool,
    pub dry_run: bool,
    pub claim_amount: i64,
    pub cooldown_seconds: i64,
}

#[derive(Deserialize)]
pub struct ClaimRequest {
    pub address: String,
}

#[derive(Serialize)]
pub struct ClaimErrResponse {
    pub ok: bool,
    pub error: &'static str,
}

/// Validates a user-supplied address string for this MVP.
///
/// This is **not** Trilogicon-specific address or bech32/checksum validation.
/// Exact address rules should later come from core-compatible logic or a shared
/// validation helper reused with the node / wallet tooling.
pub fn validate_address(address: &str) -> Result<(), &'static str> {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        return Err("address must not be empty");
    }
    if trimmed.len() > MAX_ADDRESS_LEN {
        return Err("address exceeds maximum length");
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("address must not contain control characters");
    }
    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "trilogicon-faucet",
        dry_run: state.config.dry_run,
    })
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        ok: true,
        dry_run: state.config.dry_run,
        claim_amount: state.config.claim_amount,
        cooldown_seconds: state.config.cooldown_seconds,
    })
}

async fn claim(
    State(state): State<AppState>,
    Json(body): Json<ClaimRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ClaimErrResponse>)> {
    if !state.config.dry_run {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ClaimErrResponse {
                ok: false,
                error: "payouts_not_enabled",
            }),
        ));
    }

    validate_address(&body.address).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ClaimErrResponse {
                ok: false,
                error: "invalid_address",
            }),
        )
    })?;

    let address = body.address.trim().to_string();

    if let Some(last) = db::latest_claim_time(&state.pool, &address)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ClaimErrResponse {
                    ok: false,
                    error: "database_error",
                }),
            )
        })?
    {
        let elapsed = chrono::Utc::now().signed_duration_since(last);
        if elapsed.num_seconds() < state.config.cooldown_seconds {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(ClaimErrResponse {
                    ok: false,
                    error: "rate_limited",
                }),
            ));
        }
    }

    // Client IP can be wired later (e.g. `ConnectInfo` or `X-Forwarded-For` behind a proxy).
    db::insert_claim(
        &state.pool,
        &address,
        None,
        state.config.claim_amount,
        true,
        "dry_run_accepted",
    )
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ClaimErrResponse {
                ok: false,
                error: "database_error",
            }),
        )
    })?;

    let body = serde_json::json!({
        "ok": true,
        "dry_run": true,
        "message": "claim accepted in dry-run mode",
        "address": address,
        "amount": state.config.claim_amount,
        "tx_hash": serde_json::Value::Null,
    });

    Ok(Json(body))
}

pub fn app_router(state: AppState) -> Router {
    let claim = Router::new()
        .route("/api/claim", post(claim))
        .layer(DefaultBodyLimit::max(MAX_CLAIM_BODY_BYTES));

    Router::new()
        .route("/api/health", get(health))
        .route("/api/status", get(status))
        .merge(claim)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    async fn test_app(dry_run: bool, cooldown_seconds: i64) -> (Router, sqlx::SqlitePool) {
        let pool = db::connect("sqlite::memory:").await.expect("connect");
        let config = Arc::new(Config {
            bind_addr: "127.0.0.1:0".to_string(),
            database_url: "sqlite::memory:".to_string(),
            dry_run,
            claim_amount: 10,
            cooldown_seconds,
        });
        let pool_for_assert = pool.clone();
        let app = app_router(AppState { config, pool });
        (app, pool_for_assert)
    }

    async fn claim_count(pool: &sqlx::SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM claims")
            .fetch_one(pool)
            .await
            .expect("count claims")
    }

    #[test]
    fn validate_address_rejects_empty() {
        assert!(validate_address("").is_err());
        assert!(validate_address("   ").is_err());
    }

    #[test]
    fn validate_address_rejects_control_chars() {
        assert!(validate_address("abc\u{0}def").is_err());
        assert!(validate_address("x\ny").is_err());
    }

    #[test]
    fn validate_address_accepts_reasonable_input() {
        assert!(validate_address("tl1test_something_reasonable").is_ok());
    }

    #[test]
    fn validate_address_rejects_excessive_length() {
        let long = "a".repeat(MAX_ADDRESS_LEN + 1);
        assert!(validate_address(&long).is_err());
    }

    #[tokio::test]
    async fn dry_run_disabled_returns_payouts_not_enabled() {
        let (app, pool) = test_app(false, 3600).await;
        assert_eq!(claim_count(&pool).await, 0);

        let req = Request::builder()
            .method("POST")
            .uri("/api/claim")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"address":"tl1test_example"}"#))
            .expect("request");
        let resp = app.oneshot(req).await.expect("claim");
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"], "payouts_not_enabled");
        assert_eq!(claim_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn rate_limit_applies_to_trimmed_address() {
        let (app, _pool) = test_app(true, 3600).await;

        let req = Request::builder()
            .method("POST")
            .uri("/api/claim")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"address":"  abc123  "}"#))
            .expect("request");
        let resp = app.clone().oneshot(req).await.expect("first claim");
        assert_eq!(resp.status(), StatusCode::OK);

        let req2 = Request::builder()
            .method("POST")
            .uri("/api/claim")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"address":"abc123"}"#))
            .expect("request");
        let resp2 = app.oneshot(req2).await.expect("second claim");
        assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);

        let body = to_bytes(resp2.into_body(), usize::MAX).await.expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["error"], "rate_limited");
    }

    #[tokio::test]
    async fn rate_limit_second_claim_within_cooldown() {
        let (app, _pool) = test_app(true, 3600).await;

        let req = Request::builder()
            .method("POST")
            .uri("/api/claim")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"address":"same_addr"}"#))
            .expect("request");
        let resp = app.clone().oneshot(req).await.expect("first claim");
        assert_eq!(resp.status(), StatusCode::OK);

        let req2 = Request::builder()
            .method("POST")
            .uri("/api/claim")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"address":"same_addr"}"#))
            .expect("request");
        let resp2 = app.oneshot(req2).await.expect("second claim");
        assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
