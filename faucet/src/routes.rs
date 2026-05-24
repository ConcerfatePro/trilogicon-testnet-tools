//! HTTP routes for the faucet API.

use crate::config::Config;
use crate::db;
use crate::payout::{PayoutAdapter, PayoutError, PayoutRequest};
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

const MAX_ADDRESS_LEN: usize = 256;
/// Small cap for `{"address":"..."}`; real validation stays minimal for this MVP.
const MAX_CLAIM_BODY_BYTES: usize = 512;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: SqlitePool,
    pub payout: Arc<dyn PayoutAdapter>,
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
    pub ip_cooldown_seconds: i64,
    pub payouts_enabled: bool,
    pub network: String,
    pub node_mode: String,
}

#[derive(Deserialize)]
pub struct ClaimRequest {
    pub address: String,
}

#[derive(Debug, Serialize)]
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

fn within_cooldown(last: DateTime<Utc>, cooldown_seconds: i64) -> bool {
    let elapsed = Utc::now().signed_duration_since(last);
    elapsed.num_seconds() < cooldown_seconds
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
        ip_cooldown_seconds: state.config.ip_cooldown_seconds,
        payouts_enabled: state.config.payouts_enabled_status(),
        network: state.config.network.as_str().to_string(),
        node_mode: state.config.node_mode.as_str().to_string(),
    })
}

/// Core claim handler; `client_ip` is supplied by the HTTP layer or tests.
pub(crate) async fn process_claim(
    state: &AppState,
    client_ip: &str,
    body: ClaimRequest,
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

    if let Some(last) = db::latest_claim_for_address(&state.pool, &address)
        .await
        .map_err(|_| db_error())?
    {
        if within_cooldown(last, state.config.cooldown_seconds) {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(ClaimErrResponse {
                    ok: false,
                    error: "rate_limited",
                }),
            ));
        }
    }

    if let Some(last) = db::latest_claim_for_ip(&state.pool, client_ip)
        .await
        .map_err(|_| db_error())?
    {
        if within_cooldown(last, state.config.ip_cooldown_seconds) {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(ClaimErrResponse {
                    ok: false,
                    error: "ip_rate_limited",
                }),
            ));
        }
    }
    let payout_request = PayoutRequest {
        address: address.clone(),
        amount: state.config.claim_amount as u64,
        fee: state.config.fixed_fee as u64,
        dry_run: true,
    };

    let payout_result = state
        .payout
        .submit_payout(payout_request)
        .await
        .map_err(map_payout_error)?;

    db::insert_claim(
        &state.pool,
        &address,
        Some(client_ip),
        state.config.claim_amount,
        payout_result.dry_run,
        &payout_result.status,
    )
    .await
    .map_err(|_| db_error())?;

    let tx_hash = payout_result
        .tx_hash
        .map(serde_json::Value::String)
        .unwrap_or(serde_json::Value::Null);

    let body = serde_json::json!({
        "ok": true,
        "dry_run": payout_result.dry_run,
        "message": "claim accepted in dry-run mode",
        "address": address,
        "amount": state.config.claim_amount,
        "tx_hash": tx_hash,
    });

    Ok(Json(body))
}

fn db_error() -> (StatusCode, Json<ClaimErrResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ClaimErrResponse {
            ok: false,
            error: "database_error",
        }),
    )
}

fn map_payout_error(err: PayoutError) -> (StatusCode, Json<ClaimErrResponse>) {
    match err {
        PayoutError::Disabled | PayoutError::Misconfigured => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ClaimErrResponse {
                ok: false,
                error: "payouts_not_enabled",
            }),
        ),
        PayoutError::Rejected => (
            StatusCode::BAD_REQUEST,
            Json(ClaimErrResponse {
                ok: false,
                error: "invalid_address",
            }),
        ),
        PayoutError::BackendUnavailable | PayoutError::Internal => db_error(),
    }
}

async fn claim(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<ClaimRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ClaimErrResponse>)> {
    // Local/dev: use the socket peer IP from ConnectInfo.
    // Reverse-proxy support (e.g. trusted X-Forwarded-For) should be added explicitly later;
    // do not trust X-Forwarded-For in this MVP.
    let client_ip = peer.ip().to_string();
    process_claim(&state, &client_ip, body).await
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
    use crate::payout::default_payout_adapter;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_config(dry_run: bool, cooldown_seconds: i64, ip_cooldown_seconds: i64) -> Arc<Config> {
        Arc::new(Config::test_defaults(
            dry_run,
            cooldown_seconds,
            ip_cooldown_seconds,
        ))
    }

    async fn test_app(
        dry_run: bool,
        cooldown_seconds: i64,
        ip_cooldown_seconds: i64,
    ) -> (Router, AppState) {
        let pool = db::connect("sqlite::memory:").await.expect("connect");
        let config = test_config(dry_run, cooldown_seconds, ip_cooldown_seconds);
        let state = AppState {
            config: config.clone(),
            pool,
            payout: default_payout_adapter(),
        };
        let app = app_router(state.clone());
        (app, state)
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
    async fn status_includes_safe_payout_config_fields() {
        let (app, _) = test_app(true, 3600, 7200).await;
        let req = Request::builder()
            .uri("/api/status")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("status");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["ok"], true);
        assert_eq!(json["ip_cooldown_seconds"], 7200);
        assert_eq!(json["cooldown_seconds"], 3600);
        assert_eq!(json["payouts_enabled"], false);
        assert_eq!(json["network"], "testnet");
        assert_eq!(json["node_mode"], "disabled");
        assert!(json.get("wallet_seed_path").is_none());
        assert!(json.get("node_cli_path").is_none());
        assert!(json.get("node_data_dir").is_none());
    }

    #[tokio::test]
    async fn status_excludes_configured_secret_paths() {
        let pool = db::connect("sqlite::memory:").await.expect("connect");
        let mut config = Config::test_defaults(true, 3600, 3600);
        config.wallet_seed_path = Some("/etc/trilogicon/faucet/wallet.seed".to_string());
        config.node_cli_path = Some("/usr/local/bin/trilogicon".to_string());
        config.node_data_dir = Some("/var/lib/trilogicon-testnet".to_string());
        let app = app_router(AppState {
            config: Arc::new(config),
            pool,
            payout: default_payout_adapter(),
        });

        let req = Request::builder()
            .uri("/api/status")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("status");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(json.get("wallet_seed_path").is_none());
        assert!(json.get("node_cli_path").is_none());
        assert!(json.get("node_data_dir").is_none());
    }

    #[tokio::test]
    async fn claim_returns_unchanged_dry_run_response() {
        let (_, state) = test_app(true, 3600, 3600).await;

        let Json(body) = process_claim(
            &state,
            "10.0.0.99",
            ClaimRequest {
                address: "tl1test_example".to_string(),
            },
        )
        .await
        .expect("claim");

        assert_eq!(
            body,
            serde_json::json!({
                "ok": true,
                "dry_run": true,
                "message": "claim accepted in dry-run mode",
                "address": "tl1test_example",
                "amount": 10,
                "tx_hash": serde_json::Value::Null,
            })
        );
    }

    #[tokio::test]
    async fn claim_uses_dry_run_adapter_status_in_db() {
        let (_, state) = test_app(true, 3600, 3600).await;

        let _ = process_claim(
            &state,
            "10.0.0.50",
            ClaimRequest {
                address: "adapter_test_addr".to_string(),
            },
        )
        .await
        .expect("claim");

        let status: String = sqlx::query_scalar("SELECT status FROM claims LIMIT 1")
            .fetch_one(&state.pool)
            .await
            .expect("status");
        assert_eq!(status, "dry_run_accepted");

        let dry_run: i64 = sqlx::query_scalar("SELECT dry_run FROM claims LIMIT 1")
            .fetch_one(&state.pool)
            .await
            .expect("dry_run");
        assert_eq!(dry_run, 1);
    }

    #[tokio::test]
    async fn dry_run_disabled_returns_payouts_not_enabled() {
        let (_, state) = test_app(false, 3600, 3600).await;
        assert_eq!(claim_count(&state.pool).await, 0);

        let result = process_claim(
            &state,
            "127.0.0.1",
            ClaimRequest {
                address: "tl1test_example".to_string(),
            },
        )
        .await;
        assert!(result.is_err());
        let (status, Json(err)) = result.expect_err("should fail");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(err.error, "payouts_not_enabled");
        assert_eq!(claim_count(&state.pool).await, 0);
    }

    #[tokio::test]
    async fn rate_limit_applies_to_trimmed_address() {
        let (_, state) = test_app(true, 3600, 3600).await;
        let ip = "10.0.0.1";

        let _ = process_claim(
            &state,
            ip,
            ClaimRequest {
                address: "  abc123  ".to_string(),
            },
        )
        .await
        .expect("first claim");

        let result = process_claim(
            &state,
            ip,
            ClaimRequest {
                address: "abc123".to_string(),
            },
        )
        .await;
        let (status, Json(err)) = result.expect_err("second claim");
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(err.error, "rate_limited");
    }

    #[tokio::test]
    async fn rate_limit_second_claim_within_cooldown() {
        let (_, state) = test_app(true, 3600, 3600).await;
        let ip = "10.0.0.2";

        let _ = process_claim(
            &state,
            ip,
            ClaimRequest {
                address: "same_addr".to_string(),
            },
        )
        .await
        .expect("first claim");

        let result = process_claim(
            &state,
            ip,
            ClaimRequest {
                address: "same_addr".to_string(),
            },
        )
        .await;
        let (status, _) = result.expect_err("second claim");
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn same_ip_hits_ip_rate_limited_for_different_addresses() {
        let (_, state) = test_app(true, 3600, 3600).await;
        let ip = "198.51.100.42";

        let _ = process_claim(
            &state,
            ip,
            ClaimRequest {
                address: "addr_a".to_string(),
            },
        )
        .await
        .expect("first claim");

        let result = process_claim(
            &state,
            ip,
            ClaimRequest {
                address: "addr_b".to_string(),
            },
        )
        .await;
        let (status, Json(err)) = result.expect_err("second claim same ip");
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(err.error, "ip_rate_limited");
    }

    #[tokio::test]
    async fn different_ips_can_claim_different_addresses() {
        let (_, state) = test_app(true, 3600, 3600).await;

        let _ = process_claim(
            &state,
            "203.0.113.10",
            ClaimRequest {
                address: "addr_x".to_string(),
            },
        )
        .await
        .expect("claim from ip 10");

        let _ = process_claim(
            &state,
            "203.0.113.11",
            ClaimRequest {
                address: "addr_y".to_string(),
            },
        )
        .await
        .expect("claim from ip 11");

        assert_eq!(claim_count(&state.pool).await, 2);
    }
}
