//! SQLite persistence for faucet claims.

use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;

const MIGRATION_001_VERSION: i64 = 1;
const MIGRATION_001_NAME: &str = "add_payout_tracking_columns";

const MIGRATION_001_COLUMN_DDLS: &[(&str, &str)] = &[
    ("tx_hash", "ALTER TABLE claims ADD COLUMN tx_hash TEXT NULL"),
    ("fee", "ALTER TABLE claims ADD COLUMN fee INTEGER NULL"),
    (
        "error_code",
        "ALTER TABLE claims ADD COLUMN error_code TEXT NULL",
    ),
    ("backend", "ALTER TABLE claims ADD COLUMN backend TEXT NULL"),
    (
        "idempotency_key",
        "ALTER TABLE claims ADD COLUMN idempotency_key TEXT NULL",
    ),
    (
        "attempt_count",
        "ALTER TABLE claims ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "queued_at",
        "ALTER TABLE claims ADD COLUMN queued_at TEXT NULL",
    ),
    (
        "confirmed_at",
        "ALTER TABLE claims ADD COLUMN confirmed_at TEXT NULL",
    ),
    (
        "updated_at",
        "ALTER TABLE claims ADD COLUMN updated_at TEXT NULL",
    ),
    (
        "worker_id",
        "ALTER TABLE claims ADD COLUMN worker_id TEXT NULL",
    ),
    (
        "worker_started_at",
        "ALTER TABLE claims ADD COLUMN worker_started_at TEXT NULL",
    ),
];

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
    create_base_claims_table(pool).await?;
    create_schema_migrations_table(pool).await?;
    run_migrations(pool).await?;
    Ok(())
}

async fn create_base_claims_table(pool: &SqlitePool) -> anyhow::Result<()> {
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

async fn create_schema_migrations_table(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn is_migration_applied(pool: &SqlitePool, version: i64) -> anyhow::Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations WHERE version = ?1")
            .bind(version)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

async fn record_migration(pool: &SqlitePool, version: i64, name: &str) -> anyhow::Result<()> {
    let applied_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    sqlx::query(
        r#"
        INSERT INTO schema_migrations (version, name, applied_at)
        VALUES (?1, ?2, ?3)
        "#,
    )
    .bind(version)
    .bind(name)
    .bind(applied_at)
    .execute(pool)
    .await?;
    Ok(())
}

async fn claims_column_names(pool: &SqlitePool) -> anyhow::Result<HashSet<String>> {
    let rows = sqlx::query("PRAGMA table_info(claims)")
        .fetch_all(pool)
        .await?;
    let mut names = HashSet::new();
    for row in rows {
        let name: String = row.try_get("name")?;
        names.insert(name);
    }
    Ok(names)
}

async fn add_column_if_missing(
    pool: &SqlitePool,
    columns: &mut HashSet<String>,
    name: &str,
    ddl: &str,
) -> anyhow::Result<()> {
    if columns.contains(name) {
        return Ok(());
    }
    sqlx::query(ddl).execute(pool).await?;
    columns.insert(name.to_string());
    Ok(())
}

/// Migration 001: payout-tracking columns and indexes.
///
/// Idempotent: skips columns that already exist (via `PRAGMA table_info`). If all payout
/// columns are present but the migration row was never recorded (e.g. manual schema edit),
/// indexes are ensured and the migration row is backfilled.
async fn apply_migration_001(pool: &SqlitePool) -> anyhow::Result<()> {
    let mut columns = claims_column_names(pool).await?;

    for (name, ddl) in MIGRATION_001_COLUMN_DDLS {
        add_column_if_missing(pool, &mut columns, name, ddl).await?;
    }

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_claims_address_created
            ON claims (address, created_at DESC);
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_claims_ip_created
            ON claims (ip, created_at DESC);
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_claims_status_created
            ON claims (status, created_at);
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_claims_idempotency_key
            ON claims (idempotency_key)
            WHERE idempotency_key IS NOT NULL;
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_claims_tx_hash
            ON claims (tx_hash)
            WHERE tx_hash IS NOT NULL;
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    if !is_migration_applied(pool, MIGRATION_001_VERSION).await? {
        apply_migration_001(pool).await?;
        record_migration(pool, MIGRATION_001_VERSION, MIGRATION_001_NAME).await?;
    }
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
    use sqlx::sqlite::SqliteConnectOptions;

    async fn test_pool() -> SqlitePool {
        connect("sqlite::memory:").await.expect("connect")
    }

    async fn pool_without_migrations() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::from_str("sqlite::memory:")
                    .expect("options")
                    .create_if_missing(true),
            )
            .await
            .expect("connect");
        create_base_claims_table(&pool).await.expect("base table");
        pool
    }

    async fn migration_001_applied(pool: &SqlitePool) -> bool {
        is_migration_applied(pool, MIGRATION_001_VERSION)
            .await
            .expect("query migration")
    }

    async fn index_names(pool: &SqlitePool) -> HashSet<String> {
        let rows = sqlx::query(
            r#"
            SELECT name FROM sqlite_master
            WHERE type = 'index' AND tbl_name = 'claims'
            "#,
        )
        .fetch_all(pool)
        .await
        .expect("list indexes");
        rows.iter()
            .map(|row| row.get::<String, _>("name"))
            .collect()
    }

    #[tokio::test]
    async fn fresh_db_has_schema_migrations_table() {
        let pool = test_pool().await;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn fresh_db_records_migration_001() {
        let pool = test_pool().await;
        assert!(migration_001_applied(&pool).await);

        let name: String =
            sqlx::query_scalar("SELECT name FROM schema_migrations WHERE version = ?1")
                .bind(MIGRATION_001_VERSION)
                .fetch_one(&pool)
                .await
                .expect("migration name");
        assert_eq!(name, MIGRATION_001_NAME);

        let applied_at: String =
            sqlx::query_scalar("SELECT applied_at FROM schema_migrations WHERE version = ?1")
                .bind(MIGRATION_001_VERSION)
                .fetch_one(&pool)
                .await
                .expect("applied_at");
        assert!(DateTime::parse_from_rfc3339(&applied_at).is_ok());
    }

    #[tokio::test]
    async fn fresh_db_has_payout_tracking_columns() {
        let pool = test_pool().await;
        let columns = claims_column_names(&pool).await.expect("columns");
        for (name, _) in MIGRATION_001_COLUMN_DDLS {
            assert!(columns.contains(*name), "missing column {name}");
        }
    }

    #[tokio::test]
    async fn fresh_db_has_payout_indexes() {
        let pool = test_pool().await;
        let indexes = index_names(&pool).await;
        for expected in [
            "idx_claims_address_created",
            "idx_claims_ip_created",
            "idx_claims_status_created",
            "idx_claims_idempotency_key",
            "idx_claims_tx_hash",
        ] {
            assert!(indexes.contains(expected), "missing index {expected}");
        }
    }

    #[tokio::test]
    async fn init_schema_is_idempotent() {
        let pool = test_pool().await;
        init_schema(&pool).await.expect("second init");
        assert!(migration_001_applied(&pool).await);

        let migration_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations WHERE version = ?1")
                .bind(MIGRATION_001_VERSION)
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(migration_count, 1);
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

    #[tokio::test]
    async fn dry_run_insert_leaves_payout_columns_null_or_default() {
        let pool = test_pool().await;
        let id = insert_claim(
            &pool,
            "tl1test_example",
            Some("127.0.0.1"),
            10,
            true,
            "dry_run_accepted",
        )
        .await
        .expect("insert");

        let row = sqlx::query(
            r#"
            SELECT tx_hash, fee, error_code, backend, idempotency_key,
                   attempt_count, queued_at, confirmed_at, updated_at,
                   worker_id, worker_started_at, dry_run, status
            FROM claims WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("row");

        let tx_hash: Option<String> = row.try_get("tx_hash").expect("tx_hash");
        let fee: Option<i64> = row.try_get("fee").expect("fee");
        let attempt_count: i64 = row.try_get("attempt_count").expect("attempt_count");
        let dry_run: i64 = row.try_get("dry_run").expect("dry_run");
        let status: String = row.try_get("status").expect("status");

        assert!(tx_hash.is_none());
        assert!(fee.is_none());
        assert_eq!(attempt_count, 0);
        assert_eq!(dry_run, 1);
        assert_eq!(status, "dry_run_accepted");
    }

    #[tokio::test]
    async fn upgrades_old_claims_table_preserving_rows() {
        let pool = pool_without_migrations().await;

        sqlx::query(
            r#"
            INSERT INTO claims (address, ip, amount, dry_run, status, created_at)
            VALUES ('legacy_addr', '198.51.100.1', 10, 1, 'dry_run_accepted', '2020-01-01T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("legacy insert");

        init_schema(&pool).await.expect("migrate");

        assert!(migration_001_applied(&pool).await);
        let columns = claims_column_names(&pool).await.expect("columns");
        for (name, _) in MIGRATION_001_COLUMN_DDLS {
            assert!(columns.contains(*name), "missing column {name}");
        }

        let address: String = sqlx::query_scalar("SELECT address FROM claims WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("address");
        assert_eq!(address, "legacy_addr");

        let status: String = sqlx::query_scalar("SELECT status FROM claims WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("status");
        assert_eq!(status, "dry_run_accepted");
    }

    #[tokio::test]
    async fn backfills_migration_row_when_columns_already_exist() {
        let pool = pool_without_migrations().await;

        for ddl in MIGRATION_001_COLUMN_DDLS {
            sqlx::query(ddl.1).execute(&pool).await.expect("add column");
        }

        init_schema(&pool).await.expect("backfill migrate");
        assert!(migration_001_applied(&pool).await);
    }
}
