# Faucet payout status model (MVP 3d-2c)

## 1. Purpose

This document defines **claim and payout status semantics** and **database state design** for the Trilogicon testnet faucet **before any real payout execution is added**.

It is a **documentation-first** milestone (MVP 3d-2c). Reading or following this note does **not** enable payouts, change `/api/claim` behavior, or add CLI execution. Implementation of status transitions, migrations, and confirmation checks remains gated behind explicit future milestones (MVP 3d-3+).

Related documents:

- [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md) — CLI adapter flow, argv shape, phased rollout
- [faucet_payout_worker_design.md](faucet_payout_worker_design.md) — worker serialization, locking, timeout handling
- [faucet_secret_config_plan.md](faucet_secret_config_plan.md) — secrets, fail-closed startup, operator checklist
- Faucet payout module: `faucet/src/payout.rs`

---

## 2. Current state

As of MVP 3d-2d, the faucet backend behaves as follows:

| Area | Behavior |
|------|----------|
| Mode | **Dry-run only** (`FAUCET_DRY_RUN=true` by default) |
| Claim insert | `status = dry_run_accepted`, `dry_run = 1` |
| `tx_hash` | **Not stored**; API returns no transaction hash |
| Payout adapter | Only `DryRunPayoutAdapter` is wired |
| CLI | `node send` is **not executed**; `build_cli_send_args` constructs argv only |
| Real TRIL | **Never sent** |
| Payout worker | **Not implemented** — design: [faucet_payout_worker_design.md](faucet_payout_worker_design.md) |
| `FAUCET_ENABLE_PAYOUTS=true` | **Fails startup** |

When real payouts are enabled in a future MVP, core `node send` will **queue** the transaction into `{data-dir}/pending_tx.tril`. It does **not** directly broadcast or confirm the transaction. A running `node run` must later drain `pending_tx.tril`, seal the transaction into a block, and sync chain state. **CLI success therefore means queued, not confirmed.**

---

## 3. Status vocabulary

All values below are stored in `claims.status` (TEXT) unless noted. Use these exact snake_case strings for consistency across DB rows, logs, and API responses.

### `dry_run_accepted`

- Dry-run claim accepted after validation and rate limits.
- No transaction sent or queued.
- `tx_hash` is null / omitted.
- **Current production behavior.**

### `payout_requested`

- Future real claim accepted and recorded **before** CLI execution attempt.
- Used when the DB row is inserted first (recommended pattern; see Section 6).
- No `tx_hash` yet; payout worker has not run or has not finished.

### `payout_queued`

- `node send` succeeded and queued the transaction into `pending_tx.tril`.
- `tx_hash` is known (parsed from CLI stdout: `Queued tx <TX_HASH> -> …`).
- Transaction is **not** confirmed or sealed on chain.
- This is the **first success state** for real payouts in MVP 3d-3; do not treat it as final settlement.

### `payout_rejected`

- Request rejected **before** CLI submission due to validation, rate limit, policy, or adapter rejection.
- Examples: invalid address, insufficient faucet balance (if checked), daily cap exceeded, cooldown hit at send time.
- No `tx_hash`; no entry added to `pending_tx.tril` by this attempt.

### `payout_backend_unavailable`

- CLI or node backend unavailable at execution time.
- Examples: binary missing, subprocess crash, timeout before parseable stdout, data dir unreadable by CLI.
- No reliable `tx_hash` from this attempt.

### `payout_misconfigured`

- Payout configuration invalid at request or execution time.
- Examples: missing CLI path, empty data dir, adapter not wired when payouts enabled.
- Distinct from transient backend unavailability; usually requires operator config fix.

### `payout_failed`

- CLI execution ran and **clearly failed** with no transaction hash produced.
- Examples: non-zero exit code with stderr indicating rejection; parse failure of success line with definitive failure exit.
- No `tx_hash` from this attempt.

### `payout_unknown`

- **Ambiguous** outcome: CLI may or may not have queued a transaction.
- Examples: subprocess timeout after partial stdout, lost connection to child process, DB update failed after possible CLI success.
- **Do not retry immediately.** Requires operator reconciliation (Section 10).
- May or may not have a `tx_hash`; if hash was parsed before ambiguity, store it but still mark unknown until reconciled.

### `payout_confirmed`

- Future state: transaction confirmed sealed in a block via node state, explorer, or confirmation worker.
- Requires later lookup — not available at `node send` return time.
- First real MVP (3d-3) may **not** implement this; queued-only reporting is acceptable initially.

### Status lifecycle (future real payouts)

```
                    ┌─────────────────┐
                    │ payout_requested │
                    └────────┬────────┘
                             │ CLI attempt
         ┌───────────────────┼───────────────────┐
         ▼                   ▼                   ▼
  payout_queued      payout_failed      payout_unknown
         │              payout_rejected   payout_backend_unavailable
         │              payout_misconfigured
         ▼
  payout_confirmed   (async, later MVP)
```

---

## 4. User-facing language

API `message` fields should be stable, non-technical, and **never leak secrets or CLI stderr**. Map internal `status` to user-visible text:

| Status | Example user message |
|--------|----------------------|
| `dry_run_accepted` | Dry-run claim accepted. No TRIL was sent. |
| `payout_requested` | Claim accepted. Payout processing. |
| `payout_queued` | Claim queued. Transaction hash: `<tx_hash>`. Waiting for node sealing. |
| `payout_confirmed` | Claim confirmed. |
| `payout_rejected` | Claim could not be processed. |
| `payout_backend_unavailable` | Payout service temporarily unavailable. Try again later. |
| `payout_misconfigured` | Payout service misconfigured. Contact operator. |
| `payout_failed` | Payout failed. Try again later. |
| `payout_unknown` | Claim status unknown. **Do not retry immediately.** Contact operator if funds do not arrive. |

**Critical distinction:** **`payout_queued` is not `payout_confirmed`.** Queued means the transaction is in `pending_tx.tril` awaiting `node run` to seal a block. Users must not assume TRIL has arrived until confirmation policy is met (future MVP or operator communication).

Dry-run responses remain unchanged in this milestone:

```json
{
  "ok": true,
  "dry_run": true,
  "message": "claim accepted in dry-run mode",
  "address": "tl1test_example",
  "amount": 10
}
```

---

## 5. Database design implications

### Current schema (MVP 3d-2c)

```sql
CREATE TABLE claims (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    address TEXT NOT NULL,
    ip TEXT,
    amount INTEGER NOT NULL,
    dry_run INTEGER NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL
);
```

This table is **sufficient for dry-run** but **insufficient for real payouts** without extension.

### Gaps for future real payouts

| Need | Current support |
|------|-----------------|
| Store `tx_hash` after queue | **Missing** — no column |
| Distinguish queued vs confirmed time | **Missing** — only `created_at` |
| Record fee paid | **Missing** |
| Stable client error code | **Missing** — only free-text `status` |
| Retry / attempt tracking | **Missing** |
| Idempotency | **Missing** |
| Backend identifier (`cli` vs `rpc`) | **Missing** |
| Status updates after insert | **Missing** — no `updated_at` |

### Future columns to consider (design only — no migration in 3d-2c)

| Column | Type | Purpose |
|--------|------|---------|
| `tx_hash` | TEXT NULL | Hash from CLI stdout when queued |
| `fee` | INTEGER NULL | Fee used for this payout |
| `error_code` | TEXT NULL | Stable machine-readable error (`payout_backend_unavailable`, …) |
| `queued_at` | TEXT NULL | When CLI reported queue success |
| `confirmed_at` | TEXT NULL | When confirmation worker observed seal |
| `attempt_count` | INTEGER DEFAULT 0 | Payout worker retry count |
| `idempotency_key` | TEXT NULL UNIQUE | Client or server key to dedupe retries |
| `backend` | TEXT NULL | `cli` or `rpc` |
| `updated_at` | TEXT NULL | Last status transition |
| `worker_started_at` | TEXT NULL | When payout worker began processing row |
| `worker_id` | TEXT NULL | Faucet process instance id for debugging |

Worker lifecycle and locking: [faucet_payout_worker_design.md](faucet_payout_worker_design.md).

**Recommendation:** Add columns in a dedicated migration milestone (3d-3 or 3d-4) together with the payout worker.

### Index considerations (future)

- `(address, created_at DESC)` — cooldown queries (partially served by `ORDER BY id DESC` today)
- `(status)` — reconciliation queries for `payout_unknown`, `payout_requested`
- `(idempotency_key)` UNIQUE — dedupe HTTP retries

---

## 6. Insert-before-send vs send-before-insert

### Insert before send

**Pros:**

- Full audit trail from first acceptance.
- Can record `payout_requested` before any side effect.
- Enables single-worker lock: only one in-flight payout per claim row.
- Easier reconciliation for `payout_unknown` (row exists even if CLI outcome unclear).

**Cons:**

- Rows may remain in `payout_requested` if worker crashes before CLI call.
- Requires background worker or request-scoped lock to complete stale rows.
- Cooldown may block user while row is still `payout_requested`.

### Send before insert

**Pros:**

- Happy path is simpler: CLI succeeds → insert final row once.
- No orphan `payout_requested` rows.

**Cons:**

- **Critical risk:** CLI queues tx into `pending_tx.tril`, then DB insert fails → **lost tracking** and high duplicate-payout risk on user retry.
- No row to attach reconciliation data if HTTP response is lost.
- Poor fit for `payout_unknown` handling.

### Recommended future approach

1. Validate address and rate limits.
2. **Insert** row with `status = payout_requested` (or use idempotency key to return existing row).
3. **Execute once** under a single payout worker / row-level lock (see [faucet_payout_worker_design.md](faucet_payout_worker_design.md)).
4. **Update** row to terminal or intermediate status (`payout_queued`, `payout_failed`, …) with `tx_hash` and timestamps.
5. For confirmation, **async worker** transitions `payout_queued` → `payout_confirmed` when chain state allows.

Never allow two concurrent CLI sends for the same claim id.

---

## 7. Duplicate payout risk

Real payouts introduce failure modes dry-run does not have.

| Scenario | Risk | Mitigation |
|----------|------|------------|
| HTTP retry after timeout | User POSTs again; may double-pay if first CLI succeeded | Idempotency key; return existing row; `payout_unknown` blocks retry |
| Browser double-click | Two concurrent requests same address | Single payout worker; DB lock on claim; cooldown |
| Same address/IP cooldown | Helps but races exist (MVP 2 limitation) | Insert-before-send + lock; extend cooldown to cover `payout_requested` |
| Server restart during payout | Row stuck `payout_requested`; CLI may have run | Startup reconciliation: inspect `pending_tx.tril` vs open rows |
| CLI timeout after queue success | Row may be `payout_unknown` but tx in queue | Parse partial stdout; reconcile via `pending_tx.tril` + tx hash |
| `pending_tx.tril` already contains tx | Re-send may duplicate nonce or queue duplicate | Never blind-retry; reconcile first |

### Recommendations

- **Single payout worker** (or equivalent row-level mutex) for CLI execution — [faucet_payout_worker_design.md](faucet_payout_worker_design.md)
- Keep **per-address and per-IP cooldowns**; consider treating `payout_requested` and `payout_unknown` as blocking further claims for that address until resolved.
- Support **idempotency key** (header or body) in a future API revision.
- **`payout_unknown` must block immediate retry** until operator or automated reconciliation clears or confirms the row.
- Document operator procedure for stale `payout_requested` rows (Section 10).

---

## 8. Queue vs confirmation

Understanding core behavior is essential for correct status semantics:

| Stage | What happened | Faucet status |
|-------|---------------|---------------|
| CLI not run | No side effect | `payout_requested`, `payout_failed`, etc. |
| `node send` success stdout | Tx signed and appended to `pending_tx.tril` | **`payout_queued`** |
| `node run` drains queue | Tx included in sealed block | **`payout_confirmed`** (future) |

Verified CLI success line:

```text
Queued tx <TX_HASH> -> <RECEIVER> amount <AMOUNT> fee <FEE> (nonce <NONCE>)
```

- This line proves **queue**, not **inclusion**.
- `node run` must drain `pending_tx.tril` and seal blocks.
- Confirmation requires **later** chain state or explorer lookup (MVP 3d-5+ or separate worker).

**First real MVP (3d-3):** Reporting `payout_queued` with `tx_hash` and message “Waiting for node sealing” is correct and sufficient. Do not imply on-chain confirmation until a confirmation checker exists.

---

## 9. API response model

### Future success — queued (not confirmed)

```json
{
  "ok": true,
  "dry_run": false,
  "status": "payout_queued",
  "tx_hash": "abc123…",
  "message": "Claim queued. Waiting for sealing.",
  "address": "tl1test_receiver",
  "amount": 10
}
```

### Future success — confirmed (later MVP)

```json
{
  "ok": true,
  "dry_run": false,
  "status": "payout_confirmed",
  "tx_hash": "abc123…",
  "message": "Claim confirmed.",
  "address": "tl1test_receiver",
  "amount": 10
}
```

### Future failure examples

```json
{
  "ok": false,
  "error": "payout_backend_unavailable",
  "message": "Payout service temporarily unavailable. Try again later."
}
```

```json
{
  "ok": false,
  "error": "payout_unknown",
  "message": "Claim status unknown. Do not retry immediately."
}
```

### HTTP status codes (future guidance)

| Condition | HTTP | Notes |
|-----------|------|-------|
| Dry-run accepted | 200 | Current behavior |
| Payout queued | 200 | `ok: true`; not 201 — idempotent claim semantics TBD |
| Rate limited | 429 | Unchanged |
| Payouts disabled | 503 | Unchanged |
| Backend unavailable | 503 | `ok: false` |
| Unknown | 503 or 202 | Prefer 503 with explicit `payout_unknown` to discourage retry |

**This milestone:** `/api/claim` response shape is **unchanged** (dry-run only).

---

## 10. Operator / reconciliation plan

When `payout_unknown` or stale `payout_requested` rows appear:

1. **Inspect claim DB** — note `id`, `address`, `status`, `created_at`, any stored `tx_hash`.
2. **Inspect `{data-dir}/pending_tx.tril`** — check whether a matching tx hash or receiver/nonce exists.
3. **Check node logs** — CLI stderr/stdout (server-side only; never expose to clients).
4. **Check chain state / explorer** (future) — determine if tx was sealed.
5. **Resolve row:**
   - If tx found in queue or chain → update to `payout_queued` or `payout_confirmed`.
   - If no tx and CLI clearly failed → update to `payout_failed`; allow user retry after cooldown policy review.
   - If still ambiguous → keep `payout_unknown`; do not auto-retry CLI send.

**Never blindly retry unknown claims.** A second `node send` may queue a duplicate transaction.

For dry-run operations today, reconciliation is trivial: all rows are `dry_run_accepted` with no `tx_hash`.

---

## 11. Non-goals

- **No real payout execution** in MVP 3d-2c
- **No DB migration** in this milestone unless separately scoped
- **No confirmation checker** or background sealing observer
- **No explorer integration**
- **No RPC integration**
- **No changes to `trilogicon-core`**
- **No change to `/api/claim`** dry-run behavior
- **No enabling** `FAUCET_ENABLE_PAYOUTS=true`

---

## 12. Done checklist

- [x] `docs/faucet_payout_status_model.md` exists (this file).
- [x] Root `README.md` links to this document.
- [x] [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md) references this document.
- [x] No runtime payout behavior changed.
- [x] `cargo fmt`, `cargo test`, `cargo clippy -- -D warnings` pass from `faucet/`.

---

## References

- [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md)
- [faucet_payout_worker_design.md](faucet_payout_worker_design.md)
- [faucet_secret_config_plan.md](faucet_secret_config_plan.md)
- Status constants (inert): `faucet/src/payout.rs` (`claim_status` module)
- Claims schema: `faucet/src/db.rs`
