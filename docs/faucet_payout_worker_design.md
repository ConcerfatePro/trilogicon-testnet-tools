# Faucet payout worker and locking design (MVP 3d-2d)

## 1. Purpose

This document defines the **future payout worker and locking strategy** for real testnet CLI payouts in the Trilogicon faucet **before any CLI execution code is added**.

It is a **documentation-first** milestone (MVP 3d-2d). Reading or following this note does **not** enable payouts, spawn subprocesses, or change `/api/claim` behavior. Worker implementation, DB migrations, and file locks remain gated behind MVP 3d-3+.

Related documents:

- [faucet_payout_status_model.md](faucet_payout_status_model.md) — status vocabulary, DB columns, queue vs confirmation
- [faucet_payout_db_design.md](faucet_payout_db_design.md) — schema expansion, migrations, indexes, idempotency
- [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md) — CLI adapter flow, argv shape, phased rollout
- [faucet_secret_config_plan.md](faucet_secret_config_plan.md) — secrets, fail-closed startup, operator checklist
- Faucet payout module: `faucet/src/payout.rs`

---

## 2. Current state

As of MVP 3d-2e:

| Area | Status |
|------|--------|
| HTTP claims | Dry-run only; `status = dry_run_accepted` |
| `PayoutAdapter` trait | Exists; only `DryRunPayoutAdapter` wired |
| `CliPayoutAdapter` | Skeleton; `submit_payout` returns `Disabled` |
| `build_cli_send_args` | Tested; matches verified `node send` syntax |
| Status model | [faucet_payout_status_model.md](faucet_payout_status_model.md) defines vocabulary |
| Real payouts | **None** — no CLI, no seeds, no TRIL |
| DB migration | **None** — future design: [faucet_payout_db_design.md](faucet_payout_db_design.md) |
| Payout worker | **Not implemented** |
| `FAUCET_ENABLE_PAYOUTS=true` | **Fails startup** |

---

## 3. Why a worker is needed

Core `node send` behavior creates constraints that synchronous HTTP handlers cannot safely satisfy alone:

| Property | Implication for faucet |
|----------|------------------------|
| Nonce from local committed state | Parallel `node send` calls can read the same nonce and race |
| `pending_tx.tril` is local queue state | Concurrent writes to the same data dir are unsafe without coordination |
| CLI loads `{data-dir}/wallet.seed` | Only one signing context should mutate queue state at a time |
| Subprocess timeouts | HTTP request may end before CLI finishes; outcome may be ambiguous |
| Retries after timeout | Blind retry can queue duplicate transactions |

A **single payout worker** (or equivalent serialization) provides:

- **One CLI execution at a time** per faucet instance and node data directory
- **Auditable state transitions** in SQLite (`payout_requested` → `payout_queued`, etc.)
- **Clear ownership** of which claim row is being processed
- **Controlled timeout handling** without blocking HTTP threads indefinitely
- **Reconciliation hooks** on startup for stale `payout_requested` / `payout_unknown` rows

Without a worker, parallel HTTP claim requests could invoke `node send` concurrently, creating nonce collisions, duplicate queue entries, and unrecoverable `payout_unknown` states.

---

## 4. Recommended future model

### Request path (HTTP)

1. Validate address and request body.
2. Apply per-address and per-IP cooldowns (and future daily caps).
3. Verify runtime payout gates (`dry_run` off, `enable_payouts` on, network testnet, etc.).
4. **Insert** claim row with `status = payout_requested` (or return existing row for idempotency key).
5. Enqueue work for the payout worker (in-memory channel or poll DB).
6. Return HTTP response — behavior depends on MVP stage:
   - **MVP 3d-3 (initial):** Return `202 Accepted` or `200` with `status: payout_requested` and message “Payout processing.” Client polls or waits for operator confirmation.
   - **Later:** Optionally block until worker completes (with timeout) and return `payout_queued` or failure in same response.

### Worker path (background task)

1. Dequeue next eligible `payout_requested` row (FIFO or priority policy).
2. Acquire execution locks (in-process mutex + future file lock — Section 5).
3. Mark row as in-flight (`worker_started_at`, `worker_id` — future columns).
4. Invoke `CliPayoutAdapter::submit_payout` (future MVP 3d-3) with `dry_run: false`.
5. Parse CLI stdout / exit code.
6. **Update** row atomically: `payout_queued`, `payout_failed`, `payout_unknown`, etc., plus `tx_hash`, `queued_at`, `error_code`.
7. Release locks; process next row.

### Separation of concerns

| Layer | Responsibility |
|-------|----------------|
| HTTP handler | Validation, rate limits, insert `payout_requested`, enqueue |
| Payout worker | Serialize CLI execution, update status, handle timeouts |
| Confirmation worker (later) | Poll chain/explorer; `payout_queued` → `payout_confirmed` |
| Operator | Reconcile `payout_unknown`; never blind-retry |

---

## 5. Locking strategy options

### A. In-process mutex

| Pros | Cons |
|------|------|
| Simple to implement | Only protects **one process** |
| No extra dependencies | Multiple faucet replicas can still race |
| Good for local MVP | Lost on crash without DB state |

**Use:** Required baseline for MVP 3d-3 local testing.

### B. SQLite transaction / claim row locking

| Pros | Cons |
|------|------|
| Durable audit trail | Needs schema columns (`updated_at`, `worker_id`, …) |
| Survives restarts better | `UPDATE … WHERE status = 'payout_requested'` must be atomic |
| Supports “claim row” pattern | SQLite locking semantics need careful design |

**Use:** Primary state machine backing store. Worker selects rows with `status = payout_requested` using a conditional update to avoid double-processing:

```sql
-- Illustrative; not implemented in 3d-2d
UPDATE claims SET status = 'payout_in_progress', worker_id = ?, worker_started_at = ?
WHERE id = ? AND status = 'payout_requested';
```

Consider a transient `payout_in_progress` status or use `worker_started_at IS NULL` guard (design choice for 3d-3).

### C. File lock around CLI execution

| Pros | Cons |
|------|------|
| Protects `{data-dir}/pending_tx.tril` and chain files | Does not replace DB tracking |
| Useful when CLI mutates local node data dir | Lock path and lifetime must be documented |
| Can guard against external tools touching same dir | Platform-specific lock behavior |

**Use:** Recommended **in addition to** in-process mutex when CLI execution is added. Lock file e.g. `{data-dir}/.faucet_payout.lock` or `{data-dir}/pending_tx.tril.lock` (exact path TBD with core behavior).

### D. External queue (Redis, SQS, etc.)

| Pros | Cons |
|------|------|
| Horizontally scalable | Overkill for local MVP |
| Mature retry semantics | Extra infrastructure and ops |

**Use:** Deferred until public/production deployment (MVP 3e+).

### Recommendation for local MVP (3d-3)

| Mechanism | Role |
|-----------|------|
| **One faucet process** | Single worker task; no horizontal scaling initially |
| **One in-process worker** | Tokio task or dedicated thread dequeuing payout jobs |
| **SQLite row state** | `payout_requested` insert + conditional update before CLI |
| **In-process mutex** | Ensures one CLI subprocess at a time |
| **File lock on data dir** | Protects `pending_tx.tril` during `node send` |

Do **not** run multiple payout-enabled faucet instances against the same `FAUCET_NODE_DATA_DIR` without a distributed lock (out of scope for 3d-3).

---

## 6. Future DB state machine

Canonical status strings: [faucet_payout_status_model.md](faucet_payout_status_model.md).

### Dry-run mode (current)

```
POST /api/claim → dry_run_accepted
```

No worker involvement.

### Real payout — success path

```
payout_requested → payout_queued → payout_confirmed (later async worker)
```

- `payout_queued`: CLI success; tx in `pending_tx.tril`; `tx_hash` stored.
- `payout_confirmed`: Block sealed; requires confirmation checker (later MVP).

### Real payout — failure paths

```
payout_requested → payout_rejected        (validation/policy before CLI)
payout_requested → payout_misconfigured     (config error at worker)
payout_requested → payout_backend_unavailable (CLI crash, binary missing)
payout_requested → payout_failed            (CLI clear failure, no hash)
payout_requested → payout_unknown           (ambiguous — see Section 8)
```

### States that block immediate retry

| Status | Block new claim for same address? | Notes |
|--------|-----------------------------------|-------|
| `payout_requested` | **Yes** | Worker pending or stale after crash |
| `payout_unknown` | **Yes** | Reconcile before retry |
| `payout_queued` | **Yes** (until confirmed or policy expires) | Tx already queued |
| `payout_confirmed` | Cooldown only | Normal rate limit applies |
| `payout_failed` | Cooldown only | Definitive failure; retry after cooldown if policy allows |
| `payout_rejected` | Cooldown / policy | Pre-CLI rejection |
| `payout_backend_unavailable` | Cooldown | Transient; may retry after cooldown |

Extend cooldown queries to treat open statuses (`payout_requested`, `payout_unknown`, `payout_queued`) as blocking, not only `created_at` of terminal rows.

---

## 7. Duplicate payout prevention

| Control | Purpose |
|---------|---------|
| Per-address cooldown | Limit repeat claims (existing MVP 2) |
| Per-IP cooldown | Limit abuse across addresses (existing MVP 2) |
| **Idempotency key** | Same HTTP retry returns same row; no second insert |
| **Pending payout per address** | Reject or return existing if `payout_requested` / `payout_unknown` / `payout_queued` open |
| **Pending payout per IP** | Optional stricter policy for shared NAT |
| **Worker single-flight** | Worker must not process the same `id` twice |
| **Conditional DB update** | Only transition from `payout_requested` if still in that state |
| **`payout_unknown` policy** | **No automatic retry** — operator reconciliation required |

### Worker idempotency rules

1. Each claim row is processed **at most once** by the CLI path unless operator explicitly resets after reconciliation.
2. Increment `attempt_count` only for defined retry policy (e.g. transient `payout_backend_unavailable` after operator approval — future).
3. Never auto-requeue `payout_unknown` rows on startup without inspecting `pending_tx.tril`.

---

## 8. Timeout behavior

CLI subprocess timeouts are a primary source of `payout_unknown`.

| Scenario | Recommended status | Rationale |
|----------|-------------------|-----------|
| Timeout before any stdout | `payout_backend_unavailable` or `payout_unknown` | If no evidence of queue, prefer unavailable; if dir was locked and CLI started, prefer unknown |
| Timeout after partial stdout with tx hash | `payout_unknown` | Hash may be valid but DB update uncertain |
| Process killed after success line on stdout | `payout_unknown` | Queue likely succeeded; verify `pending_tx.tril` |
| DB update fails after CLI success | `payout_unknown` | **Do not** call CLI again; reconcile |
| Clear non-zero exit, no success line | `payout_failed` | Definitive CLI rejection |
| Parse error with success exit | `payout_unknown` | Ambiguous |

**Rule:** When queue success is **plausible but unconfirmed**, prefer `payout_unknown` over `payout_failed`. `payout_failed` implies safe to retry after cooldown; `payout_unknown` implies **stop**.

Suggested worker timeout: configurable (e.g. 30–60s for local MVP); log timeout with claim `id`, never log seed or full paths.

---

## 9. Suggested future DB columns

Full column definitions, indexes, and migration strategy: **[faucet_payout_db_design.md](faucet_payout_db_design.md)**.

Summary (worker-relevant):

| Column | Type | Purpose |
|--------|------|---------|
| `tx_hash` | TEXT NULL | From CLI stdout when queued |
| `fee` | INTEGER NULL | Fee used for payout |
| `error_code` | TEXT NULL | Stable error (`payout_backend_unavailable`, …) |
| `queued_at` | TEXT NULL | When CLI reported queue success |
| `confirmed_at` | TEXT NULL | When confirmation worker observed seal |
| `attempt_count` | INTEGER DEFAULT 0 | Worker processing attempts |
| `idempotency_key` | TEXT NULL UNIQUE | Dedupe HTTP retries |
| `backend` | TEXT NULL | `cli` or `rpc` |
| `updated_at` | TEXT NULL | Last status transition |
| `worker_started_at` | TEXT NULL | When worker began processing row |
| `worker_id` | TEXT NULL | Process instance id / UUID for debugging |

**Do not implement migration in MVP 3d-2d/3d-2e.** Add with worker in MVP 3d-3 per [faucet_payout_db_design.md](faucet_payout_db_design.md).

Optional transient status `payout_in_progress` may replace `worker_started_at` guard — decide in 3d-3 implementation.

---

## 10. Worker lifecycle

### Startup

1. Load config; refuse start if payout gates fail (existing MVP 3a rules).
2. **Scan** for rows in `payout_requested` or `payout_unknown`.
3. For `payout_requested`:
   - If `worker_started_at` stale (crash recovery threshold), re-enqueue for processing **once** after checking `pending_tx.tril`.
   - If recently started, may indicate crash mid-flight → treat as `payout_unknown` until reconciled.
4. For `payout_unknown`: **do not auto-retry CLI**; log count; require operator policy.
5. Start worker loop (only when payouts enabled in future MVP).

### Steady state

- Poll DB or receive from channel; process one payout at a time.
- Update `updated_at` on every transition.

### Graceful shutdown

- Stop accepting new `payout_requested` enqueue (or finish HTTP insert but worker drains stop).
- Wait for in-flight CLI with bounded timeout.
- If CLI completes during shutdown, persist final status.
- If shutdown mid-CLI: leave row as `payout_unknown` or `payout_requested` with `worker_started_at` set; document in runbook.

### Operator policy for unknown rows

See [faucet_payout_status_model.md §10](faucet_payout_status_model.md#10-operator--reconciliation-plan): inspect DB, `pending_tx.tril`, node logs, chain state. **Never blind-retry.**

---

## 11. Observability

### Safe to log

| Field | Example |
|-------|---------|
| Claim `id` | `claim_id=42` |
| Status transition | `payout_requested → payout_queued` |
| `tx_hash` | After queue success |
| Address (optional) | Truncated: `tl1test…xyz` |
| `error_code` | `payout_backend_unavailable` |
| Worker id | UUID per process start |
| Duration | CLI wall time ms |

### Never log

- Wallet seed, mnemonic, private keys
- Full contents of `wallet.seed`
- Full secret paths (`FAUCET_WALLET_SEED_PATH`, absolute CLI paths if sensitive)
- Raw CLI stderr (may leak paths); sanitize and cap length server-side

### CLI argv logging

- Avoid logging full argv if it embeds sensitive directory paths.
- Prefer logging subcommand `send`, claim `id`, receiver address, amount, fee.

### Metrics (future)

- Claims by status
- Worker queue depth
- CLI timeout rate
- `payout_unknown` count (alert threshold)

---

## 12. Interaction with `node run`

| Step | Component | Faucet responsibility |
|------|-----------|----------------------|
| 1 | `node send` (worker invokes) | Queue tx to `pending_tx.tril`; status → `payout_queued` |
| 2 | `node run` (operator/node) | Drain queue, seal block, sync |
| 3 | Confirmation checker (later) | Observe sealed tx; status → `payout_confirmed` |

**Worker success (`payout_queued`) does not mean the user has received TRIL on chain.** Document this in API messages. Operator must ensure `node run` is running or will run later to drain `pending_tx.tril`.

If `node run` is stopped:

- Payouts may still queue via `node send`.
- Users see “Waiting for node sealing.”
- Confirmation remains pending until node drains queue.

Confirmation checker and explorer integration are **later work** (post 3d-3).

---

## 13. Non-goals

- **No command execution** in MVP 3d-2d
- **No DB migration** in this milestone
- **No payout worker implementation** in this milestone
- **No confirmation checker**
- **No public deployment hardening** (MVP 3e)
- **No frontend**
- **No changes to `trilogicon-core`**
- **No change to `/api/claim`** dry-run behavior
- **No enabling** `FAUCET_ENABLE_PAYOUTS=true`

---

## 14. Done checklist

- [x] `docs/faucet_payout_worker_design.md` exists (this file).
- [x] Root `README.md` links to this document.
- [x] [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md) references this document.
- [x] [faucet_payout_status_model.md](faucet_payout_status_model.md) references worker/locking where relevant.
- [x] No runtime behavior changed.
- [x] `cargo fmt`, `cargo test`, `cargo clippy -- -D warnings` pass from `faucet/`.

---

## References

- [faucet_payout_status_model.md](faucet_payout_status_model.md)
- [faucet_payout_db_design.md](faucet_payout_db_design.md)
- [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md)
- [faucet_secret_config_plan.md](faucet_secret_config_plan.md)
- Payout module: `faucet/src/payout.rs`
- Claims schema: `faucet/src/db.rs`
