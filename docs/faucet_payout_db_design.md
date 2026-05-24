# Faucet payout database design (MVP 3d-2e)

## 1. Purpose

This document designs the **future SQLite schema and migration strategy** needed for real testnet payout tracking in the Trilogicon faucet **before any migration code or CLI execution is added**.

It is a **documentation-first** milestone (MVP 3d-2e). **Migration 001 was implemented in MVP 3d-3a** (`faucet/src/db.rs`); columns exist but are not yet used for real payouts. Worker and CLI execution remain future work.

Related documents:

- [faucet_payout_status_model.md](faucet_payout_status_model.md) — status vocabulary, API responses, reconciliation
- [faucet_payout_worker_design.md](faucet_payout_worker_design.md) — worker serialization, row claiming, locking
- [faucet_local_payout_readiness_checklist.md](faucet_local_payout_readiness_checklist.md) — final gate before migration/execution (MVP 3d-2f)
- [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md) — CLI adapter flow, phased rollout
- [faucet_secret_config_plan.md](faucet_secret_config_plan.md) — secrets, fail-closed startup
- Current schema: `faucet/src/db.rs`

---

## 2. Current database state

As of **MVP 3d-3a**, the faucet uses SQLite with versioned migrations in `faucet/src/db.rs`.

### Base `claims` table (unchanged columns from MVP 1)

```sql
CREATE TABLE IF NOT EXISTS claims (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    address TEXT NOT NULL,
    ip TEXT,
    amount INTEGER NOT NULL,
    dry_run INTEGER NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL  -- RFC3339 UTC
);
```

### Migration tracking (`schema_migrations`, MVP 3d-3a)

```sql
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL  -- RFC3339 UTC
);
```

### Migration 001 — payout tracking columns (**implemented**)

Added via idempotent `ALTER TABLE` (each column checked with `PRAGMA table_info(claims)` before add):

| Column | Type | Default |
|--------|------|---------|
| `tx_hash` | TEXT NULL | — |
| `fee` | INTEGER NULL | — |
| `error_code` | TEXT NULL | — |
| `backend` | TEXT NULL | — |
| `idempotency_key` | TEXT NULL | — |
| `attempt_count` | INTEGER NOT NULL | `0` |
| `queued_at` | TEXT NULL | — |
| `confirmed_at` | TEXT NULL | — |
| `updated_at` | TEXT NULL | — |
| `worker_id` | TEXT NULL | — |
| `worker_started_at` | TEXT NULL | — |

Indexes (all `IF NOT EXISTS`):

- `idx_claims_address_created` — `(address, created_at DESC)`
- `idx_claims_ip_created` — `(ip, created_at DESC)`
- `idx_claims_status_created` — `(status, created_at)`
- `idx_claims_idempotency_key` — UNIQUE partial on `idempotency_key`
- `idx_claims_tx_hash` — partial on `tx_hash`

**Runtime usage today:** Dry-run inserts still write only the original columns; new columns remain NULL / default (`attempt_count = 0`). No payout worker or real payout flow uses them yet.

### Legacy note (pre-3d-3a)

Previously (MVP 3d-2e), only the base table existed with no `schema_migrations` table.

---

## 3. Why schema expansion is needed

Real payouts require **durable, queryable state** beyond a single `status` string and `created_at`:

| Need | Why current schema fails |
|------|--------------------------|
| Pending payout attempts | Cannot distinguish “accepted, not yet sent” from final outcomes |
| Transaction hash | No column; cannot link claim row to queued tx |
| Fee | Not stored; reconciliation and audit incomplete |
| Queued timestamp | Cannot tell when CLI succeeded vs when claim was created |
| Unknown states | No `attempt_count`, `worker_id`, or `updated_at` for crash recovery |
| Retries / attempts | No counter; worker cannot record processing history |
| Worker ownership | Two processes could pick same row without claim metadata |
| Idempotency | HTTP retries can insert duplicate rows |
| Backend type | Future RPC vs CLI not distinguishable |
| Reconciliation | Operators cannot efficiently query open payouts or match `pending_tx.tril` |

The expanded schema supports the worker model in [faucet_payout_worker_design.md](faucet_payout_worker_design.md) and status transitions in [faucet_payout_status_model.md](faucet_payout_status_model.md).

---

## 4. Proposed future `claims` table additions

All new columns are **nullable or defaulted** so existing dry-run rows remain valid after `ALTER TABLE`. Types match current conventions (TEXT timestamps as RFC3339 UTC, INTEGER booleans as 0/1).

| Column | Type | Default | Purpose |
|--------|------|---------|---------|
| `tx_hash` | TEXT NULL | — | Hash from CLI stdout when `node send` queues tx |
| `fee` | INTEGER NULL | — | Fee charged for this payout (from `FAUCET_FIXED_FEE`) |
| `error_code` | TEXT NULL | — | Stable machine-readable error (`payout_backend_unavailable`, …) |
| `backend` | TEXT NULL | — | `cli` or `rpc` (future) |
| `idempotency_key` | TEXT NULL | — | Dedupe key for real claims; UNIQUE when present |
| `attempt_count` | INTEGER NOT NULL | `0` | Worker processing attempts for this row |
| `queued_at` | TEXT NULL | — | When CLI reported queue success |
| `confirmed_at` | TEXT NULL | — | When confirmation worker observed on-chain seal |
| `updated_at` | TEXT NULL | — | Last status or metadata transition |
| `worker_id` | TEXT NULL | — | Faucet process instance id that claimed the row |
| `worker_started_at` | TEXT NULL | — | When worker began processing (claim lease) |

### Per-column rationale

**`tx_hash`** — Links claim to `pending_tx.tril` entry and future explorer lookup. Null until `payout_queued` or parsed during ambiguous timeout.

**`fee`** — Audit trail; must match config at send time. Helps detect config drift vs queued tx.

**`error_code`** — API-stable error string separate from free-text logs. Enables metrics and client handling without exposing CLI stderr.

**`backend`** — Which adapter executed (`cli` first). Useful when RPC is added later.

**`idempotency_key`** — Prevents duplicate rows on HTTP retry. See Section 6.

**`attempt_count`** — Tracks worker retries (only for explicitly allowed cases). Starts at `0` on insert; incremented when worker picks up row (design choice for 3d-3).

**`queued_at`** — Distinct from `created_at`; measures queue latency and supports “stuck queued” alerts.

**`confirmed_at`** — Set by async confirmation worker when tx is sealed (later MVP).

**`updated_at`** — Any status transition or reconciliation edit; essential for startup scans and operator dashboards.

**`worker_id`** — UUID or hostname+pid set at worker claim time; debug duplicate-worker bugs.

**`worker_started_at`** — Lease timestamp for crash recovery: stale lease → `payout_unknown` or re-enqueue policy.

### Optional future column (not required for 3d-3 MVP)

| Column | Purpose |
|--------|---------|
| `cli_exit_code` | Integer exit code for audit (never expose raw stderr in DB) |
| `notes` | Operator reconciliation notes (internal tool only) |

Do **not** store raw CLI stderr, seed paths, or wallet material in the database.

---

## 5. Status constraints

Allowed `status` values (canonical list): [faucet_payout_status_model.md](faucet_payout_status_model.md)

| Status | Real payout? |
|--------|--------------|
| `dry_run_accepted` | No |
| `payout_requested` | Yes (pending worker) |
| `payout_queued` | Yes (tx in `pending_tx.tril`) |
| `payout_rejected` | Yes (terminal, pre-CLI) |
| `payout_backend_unavailable` | Yes (terminal or retry policy TBD) |
| `payout_misconfigured` | Yes (terminal) |
| `payout_failed` | Yes (terminal) |
| `payout_unknown` | Yes (blocking; reconcile) |
| `payout_confirmed` | Yes (terminal success, later) |

Optional transient status `payout_in_progress` may be added in 3d-3 instead of relying solely on `worker_started_at`; document choice at implementation time.

### CHECK constraint vs application-level validation

**Option A — SQLite CHECK constraint:**

```sql
CHECK (status IN (
  'dry_run_accepted', 'payout_requested', 'payout_queued', ...
))
```

| Pros | Cons |
|------|------|
| DB rejects invalid status | Migration must update constraint when adding statuses |
| Strong integrity | Harder to evolve during early MVP iteration |

**Option B — Application-level only (recommended for local MVP 3d-3):**

| Pros | Cons |
|------|------|
| Simple migrations | Buggy code can write invalid status |
| Easy to add statuses | No DB-enforced vocabulary |

**Recommendation:** **Application-level validation** for MVP 3d-3, using constants in `faucet/src/payout.rs` (`claim_status` module). Add CHECK constraint in MVP 3d-4 or 3d-5 once vocabulary stabilizes.

---

## 6. Idempotency strategy

### Goals

- Prevent double payout when client retries POST after timeout or double-clicks.
- Survive server restart without creating a second claim row for the same logical request.
- Never embed secrets in the key.

### Server-generated key (recommended for first real MVP)

1. On first successful validation for a **real** payout claim, generate `idempotency_key = UUID v4` (or `{address}:{created_at_floor}` — UUID preferred).
2. Store in `claims.idempotency_key`.
3. **UNIQUE** index on `idempotency_key` where not null.
4. HTTP layer: optional client header `Idempotency-Key` later; for 3d-3, key is **internal only** — return in response body if useful for support tickets.

### Client-provided key (defer)

| Pros | Cons |
|------|------|
| Standard pattern for API clients | Validation, length limits, abuse |
| Retries are explicit | Clients may collide or reuse keys incorrectly |

**Recommendation:** **Server-generated first.** Add optional client `Idempotency-Key` header in MVP 3d-4 after local smoke tests.

### Retry behavior with idempotency

```
POST /api/claim (real payout mode)
  → if Idempotency-Key header matches existing row → return same row/status (200/202)
  → else insert payout_requested with new key
```

For dry-run mode, idempotency is **optional** — cooldowns already limit abuse; adding keys can wait until real payouts.

### Key properties

- Max length e.g. 64 characters.
- Charset: `[a-zA-Z0-9_-]` only.
- Must not contain address of seed, paths, or IP alone (compose with UUID).

---

## 7. Worker claiming strategy

Design aligns with [faucet_payout_worker_design.md](faucet_payout_worker_design.md).

### Select and claim (single worker, local MVP)

1. Worker polls for eligible rows:

```sql
SELECT id FROM claims
WHERE status = 'payout_requested'
ORDER BY id ASC
LIMIT 1;
```

2. **Atomic claim** (prevent double processing):

```sql
UPDATE claims
SET worker_id = ?1,
    worker_started_at = ?2,
    updated_at = ?2,
    attempt_count = attempt_count + 1
WHERE id = ?3
  AND status = 'payout_requested'
  AND worker_started_at IS NULL;
```

3. If `rows_affected == 0`, another worker claimed it or status changed — skip.
4. Execute CLI via adapter (future 3d-3).
5. Finalize:

```sql
UPDATE claims
SET status = ?1,
    tx_hash = ?2,
    queued_at = ?3,
    error_code = ?4,
    updated_at = ?5,
    worker_id = NULL,
    worker_started_at = NULL
WHERE id = ?6
  AND worker_id = ?7;
```

Use a **single transaction** wrapping steps 2–5 where possible.

### SQLite limitations

| Limitation | Mitigation |
|------------|------------|
| No `SELECT … FOR UPDATE SKIP LOCKED` (older SQLite) | Single faucet process + in-process mutex; conditional `UPDATE` |
| WAL concurrent readers | One writer for payout updates; `busy_timeout` already 5s |
| Multiple faucet replicas | **Unsupported** for 3d-3 without distributed lock |

### Stale claim recovery (startup)

If `status = payout_requested` and `worker_started_at` older than threshold (e.g. 5 minutes):

1. Do **not** blindly re-run CLI.
2. Inspect `pending_tx.tril` for matching receiver/amount.
3. Transition to `payout_queued`, `payout_unknown`, or leave for operator per runbook.

---

## 8. Migration strategy

### Implemented (MVP 3d-3a)

1. **`schema_migrations`** table with `version`, `name`, `applied_at`.
2. **Migration 001** (`add_payout_tracking_columns`) runs at startup via `init_schema` → `run_migrations`.
3. **Fail closed:** migration errors propagate from `connect()`; startup exits non-zero.
4. **Idempotent:** migration 001 skipped if version row exists; columns added only when missing (`PRAGMA table_info`); indexes use `IF NOT EXISTS`.
5. **Backfill:** if all migration 001 columns exist but the version row is missing, migration body runs (no-op column adds), indexes ensured, then version row recorded.

### Example migration 001 (implemented in Rust)

```sql
-- 001_add_payout_columns.sql (design only)
ALTER TABLE claims ADD COLUMN tx_hash TEXT NULL;
ALTER TABLE claims ADD COLUMN fee INTEGER NULL;
ALTER TABLE claims ADD COLUMN error_code TEXT NULL;
ALTER TABLE claims ADD COLUMN backend TEXT NULL;
ALTER TABLE claims ADD COLUMN idempotency_key TEXT NULL;
ALTER TABLE claims ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE claims ADD COLUMN queued_at TEXT NULL;
ALTER TABLE claims ADD COLUMN confirmed_at TEXT NULL;
ALTER TABLE claims ADD COLUMN updated_at TEXT NULL;
ALTER TABLE claims ADD COLUMN worker_id TEXT NULL;
ALTER TABLE claims ADD COLUMN worker_started_at TEXT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_claims_idempotency_key
  ON claims(idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_claims_status_created
  ON claims(status, created_at);

CREATE INDEX IF NOT EXISTS idx_claims_address_created
  ON claims(address, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_claims_ip_created
  ON claims(ip, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_claims_tx_hash
  ON claims(tx_hash) WHERE tx_hash IS NOT NULL;
```

SQLite `ALTER TABLE ADD COLUMN` is additive and preserves existing rows (dry-run history intact). See `apply_migration_001` in `faucet/src/db.rs`.

### Future migrations

Migration 002+ (worker state, confirmation fields usage, etc.) will follow the same version-guarded pattern.

### Operational guidance

| Environment | Guidance |
|-------------|----------|
| Local dev | Delete `faucet.db` and recreate if needed during 3d-3 dev |
| Staging / public | **Backup DB** before migration; test migration on copy first |
| Rollback | Keep down migrations or restore backup; do not drop columns in production hastily |

Worker and CLI execution remain **not implemented** (MVP 3d-3b+).

---

## 9. Indexes

| Index | Columns | Purpose |
|-------|---------|---------|
| Primary | `id` | Already exists |
| `idx_claims_address_created` | `(address, created_at DESC)` | Per-address cooldown; “open payout” queries per address |
| `idx_claims_ip_created` | `(ip, created_at DESC)` | Per-IP cooldown |
| `idx_claims_status_created` | `(status, created_at)` | Worker poll; reconciliation dashboards (`payout_unknown`, `payout_requested`) |
| `idx_claims_idempotency_key` | `idempotency_key` UNIQUE WHERE NOT NULL | Dedupe HTTP retries |
| `idx_claims_tx_hash` | `tx_hash` WHERE NOT NULL | Operator lookup by hash; confirmation worker |

Partial indexes (`WHERE … IS NOT NULL`) keep dry-run-only databases small.

Existing queries use `ORDER BY id DESC LIMIT 1` — address/IP indexes optimize as claim volume grows.

---

## 10. Failure / reconciliation support

How the expanded schema supports operational scenarios:

| Scenario | Schema fields used |
|----------|-------------------|
| `payout_unknown` | `status`, `tx_hash?`, `worker_started_at`, `attempt_count`, `updated_at` |
| Queued but not confirmed | `status=payout_queued`, `tx_hash`, `queued_at`; `confirmed_at` null |
| DB update failed after CLI queue | Row may show `payout_requested` with stale `worker_started_at` or partial `tx_hash` → mark `payout_unknown` on reconcile |
| Restart recovery | Scan `status IN ('payout_requested','payout_unknown')`; compare `tx_hash` / address to `pending_tx.tril` |
| Manual reconciliation | Operator updates `status`, `confirmed_at`, `notes` via internal tool; always set `updated_at` |
| Auditing | `created_at`, `queued_at`, `confirmed_at`, `attempt_count`, `backend`, `fee`, `error_code` |

Join claim row → `tx_hash` → node `pending_tx.tril` → chain explorer (later) without re-running CLI.

---

## 11. Privacy / logging considerations

| Data | Treatment |
|------|-----------|
| `ip` | Already stored (MVP 2); treat as sensitive operations data |
| `address` | Public-ish; still minimize in logs (truncation optional) |
| `tx_hash` | Public on chain once sealed; safe in API responses; do not log alongside full IP in shared logs if avoidable |
| `error_code` | Generic stable strings only |
| CLI stderr | **Do not store** in DB; sanitize if logged server-side |
| Secret paths | Never store `FAUCET_WALLET_SEED_PATH`, CLI paths, or genesis paths in claims table |
| Seeds / keys | **Never** store |

Database backups (`faucet.db`) contain IPs and addresses — restrict file permissions; ignore in git (already in `.gitignore`).

---

## 12. Non-goals

- **No DB migration** in MVP 3d-2e
- **No change** to `faucet/src/db.rs` schema
- **No command execution**
- **No real payouts**
- **No confirmation checker**
- **No explorer integration**
- **No changes to `trilogicon-core`**
- **No change to `/api/claim`** runtime behavior

---

## 13. Done checklist

- [x] `docs/faucet_payout_db_design.md` exists (this file).
- [x] Root `README.md` links to this document.
- [x] [faucet_payout_worker_design.md](faucet_payout_worker_design.md) references this document.
- [x] [faucet_payout_status_model.md](faucet_payout_status_model.md) references this document.
- [x] No runtime behavior changed.
- [x] No schema changed in code.
- [x] `cargo fmt`, `cargo test`, `cargo clippy -- -D warnings` pass from `faucet/`.

---

## References

- [faucet_payout_status_model.md](faucet_payout_status_model.md)
- [faucet_payout_worker_design.md](faucet_payout_worker_design.md)
- [faucet_local_payout_readiness_checklist.md](faucet_local_payout_readiness_checklist.md)
- [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md)
- Current implementation: `faucet/src/db.rs`
