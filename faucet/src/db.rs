//! SQLite persistence for faucet claims.

use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use std::time::Duration;

pub async fn connect(database_url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    init_schema(&pool).await?;
    Ok(pool)
}

async fn init_schema(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS claims (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            address TEXT NOT NULL,
            ip TEXT,
            amount INTEGER NOT NULL,
            dry_run INTEGER NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

fn parse_created_at(row: &sqlx::sqlite::SqliteRow) -> Result<DateTime<Utc>, sqlx::Error> {
    let created_at: String = row.try_get("created_at")?;
    DateTime::parse_from_rfc3339(&created_at)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| sqlx::error::Error::Decode(Box::new(e)))
}

/// Returns the `created_at` of the most recent claim for this exact address, if any.
pub async fn latest_claim_for_address(
    pool: &SqlitePool,
    address: &str,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT created_at
        FROM claims
        WHERE address = ?1
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .bind(address)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(row) => Ok(Some(parse_created_at(&row)?)),
        None => Ok(None),
    }
}

/// Returns the `created_at` of the most recent claim for this client IP, if any.
pub async fn latest_claim_for_ip(
    pool: &SqlitePool,
    ip: &str,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT created_at
        FROM claims
        WHERE ip = ?1
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .bind(ip)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(row) => Ok(Some(parse_created_at(&row)?)),
        None => Ok(None),
    }
}

pub async fn insert_claim(
    pool: &SqlitePool,
    address: &str,
    ip: Option<&str>,
    amount: i64,
    dry_run: bool,
    status: &str,
) -> Result<i64, sqlx::Error> {
    let created_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let dry_run_int: i64 = if dry_run { 1 } else { 0 };

    let r = sqlx::query(
        r#"
        INSERT INTO claims (address, ip, amount, dry_run, status, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
    )
    .bind(address)
    .bind(ip)
    .bind(amount)
    .bind(dry_run_int)
    .bind(status)
    .bind(created_at)
    .execute(pool)
    .await?;

    Ok(r.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        connect("sqlite::memory:").await.expect("connect")
    }

    #[tokio::test]
    async fn insert_and_latest() {
        let pool = test_pool().await;
        insert_claim(&pool, "addr1", None, 10, true, "dry_run_accepted")
            .await
            .expect("insert");
        let t = latest_claim_for_address(&pool, "addr1")
            .await
            .expect("query");
        assert!(t.is_some());
        assert!(latest_claim_for_address(&pool, "addr2")
            .await
            .expect("q")
            .is_none());

        insert_claim(
            &pool,
            "addr2",
            Some("203.0.113.1"),
            10,
            true,
            "dry_run_accepted",
        )
        .await
        .expect("insert with ip");
        let ip_t = latest_claim_for_ip(&pool, "203.0.113.1")
            .await
            .expect("ip query");
        assert!(ip_t.is_some());
        assert!(latest_claim_for_ip(&pool, "203.0.113.2")
            .await
            .expect("ip q")
            .is_none());
    }
}
