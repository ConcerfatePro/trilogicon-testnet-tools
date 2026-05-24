# Local payout readiness checklist (MVP 3d-2f)

## 1. Purpose

This document is the **final readiness checklist** that must be satisfied before **gated local CLI payout execution** is added to the Trilogicon testnet faucet.

It is a **documentation-only** milestone (MVP 3d-2f). Completing or reading this checklist does **not** enable payouts, run migrations, or execute `node send`. It consolidates prerequisites from prior design notes into a single operator and implementer gate.

Related documents:

- [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md) — CLI adapter flow, phased rollout
- [faucet_payout_status_model.md](faucet_payout_status_model.md) — status vocabulary, queue vs confirmation
- [faucet_payout_worker_design.md](faucet_payout_worker_design.md) — worker, locking, timeouts
- [faucet_payout_db_design.md](faucet_payout_db_design.md) — schema, migrations, idempotency
- [faucet_secret_config_plan.md](faucet_secret_config_plan.md) — secrets, fail-closed startup

---

## 2. Current state summary

The following milestones are **complete** (design and/or code as noted). The faucet remains **dry-run only**.

| Milestone | Deliverable | Executes CLI? |
|-----------|-------------|---------------|
| MVP 1 | Dry-run backend (Axum, SQLite, `/api/claim`) | No |
| MVP 2 | Per-address and per-IP rate limits | No |
| MVP 3 | Secret/config safety plan | No |
| MVP 3a | Payout config gates (`FAUCET_ENABLE_PAYOUTS=true` fails startup) | No |
| MVP 3b | Wallet seed path boundary validation (file never opened) | No |
| MVP 3c | `PayoutAdapter` trait + `DryRunPayoutAdapter` wired | No |
| MVP 3d-prep | Local testnet payout design note | No |
| MVP 3d-1 | Disabled `CliPayoutAdapter` skeleton | No |
| MVP 3d-2 | CLI argv construction hardening | No |
| MVP 3d-2b | Argv builder aligned with verified `node send` syntax | No |
| MVP 3d-2c | Payout status model | No |
| MVP 3d-2d | Payout worker / locking design | No |
| MVP 3d-2e | Payout DB schema / migration design | No |
| MVP 3d-2f | This readiness checklist | No |
| **MVP 3d-3a** | **DB migration 001 + payout columns (unused)** | No |

**Not yet implemented:** payout worker, CLI subprocess, real payouts, seed reading by faucet, `payout_requested` claim flow.

---

## 3. Hard blockers before execution

All items below must be **implemented and tested** before local CLI execution (MVP 3d-3). None are satisfied today.

| # | Blocker | Reference |
|---|---------|-----------|
| 1 | ~~DB migration `001` implemented and tested~~ | **Done (MVP 3d-3a)** — [faucet_payout_db_design.md](faucet_payout_db_design.md) |
| 2 | `payout_requested` row insertion path in claim handler | [faucet_payout_status_model.md](faucet_payout_status_model.md) |
| 3 | Single payout worker (one payout at a time) | [faucet_payout_worker_design.md](faucet_payout_worker_design.md) |
| 4 | Worker row claiming (conditional `UPDATE`) | [faucet_payout_db_design.md §7](faucet_payout_db_design.md#7-worker-claiming-strategy) |
| 5 | File lock around node data dir during CLI | [faucet_payout_worker_design.md §5](faucet_payout_worker_design.md#5-locking-strategy-options) |
| 6 | Subprocess timeout policy → `payout_unknown` when ambiguous | [faucet_payout_worker_design.md §8](faucet_payout_worker_design.md#8-timeout-behavior) |
| 7 | Stdout tx hash parser implemented and unit-tested | Verified line below |
| 8 | Startup gates updated for real payout mode (allow `FAUCET_ENABLE_PAYOUTS=true` only when all gates pass) | [faucet_secret_config_plan.md](faucet_secret_config_plan.md) |
| 9 | Local testnet `node run` workflow documented for operators | Section 9 below |
| 10 | Operator acknowledges **queued ≠ confirmed** | [faucet_payout_status_model.md](faucet_payout_status_model.md) |

---

## 4. Required config checklist

Future local CLI payout requires **all** of the following. Current code **still rejects** `FAUCET_ENABLE_PAYOUTS=true` at startup until MVP 3d-3 gates are implemented.

| Variable | Required value | Notes |
|----------|----------------|-------|
| `FAUCET_DRY_RUN` | `false` | Must pair with enable switch |
| `FAUCET_ENABLE_PAYOUTS` | `true` | **Fails startup today** |
| `FAUCET_NETWORK` | `testnet` | Non-testnet must fail |
| `FAUCET_NODE_MODE` | `cli` | RPC not in first local MVP |
| `FAUCET_NODE_CLI_PATH` | Absolute path to `node` binary | Validated at startup |
| `FAUCET_NODE_DATA_DIR` | Node data directory | Must contain faucet `wallet.seed` |
| `FAUCET_GENESIS_PATH` or config equivalent | Optional | Core defaults to `{data-dir}/genesis.toml` |
| `FAUCET_FIXED_FEE` | `> 0` | Parsed today |
| `FAUCET_CLAIM_AMOUNT` | `> 0` | Parsed today |
| `FAUCET_COOLDOWN_SECONDS` | `> 0` | Required when payouts enabled |
| `FAUCET_IP_COOLDOWN_SECONDS` | `> 0` | Required when payouts enabled |
| `FAUCET_MAX_DAILY_CLAIMS` | `> 0` | Enforced at send time (future) |
| `FAUCET_MAX_DAILY_AMOUNT` | `> 0` | Enforced at send time (future) |

### Contradictions that must fail startup

- `FAUCET_DRY_RUN=true` and `FAUCET_ENABLE_PAYOUTS=true`
- `FAUCET_DRY_RUN=false` and `FAUCET_ENABLE_PAYOUTS=false`
- Payouts enabled with zero cooldowns
- `FAUCET_NODE_MODE=cli` without CLI path or data dir

---

## 5. Wallet / data-dir checklist

Core `node send` uses the **node data directory** for signing and chain state. The faucet does not pass a separate seed flag.

| Item | Requirement |
|------|-------------|
| `wallet.seed` location | `{FAUCET_NODE_DATA_DIR}/wallet.seed` — loaded by **core CLI**, not by faucet code in early MVP |
| Faucet repo | Must **not** contain `wallet.seed`; path outside git |
| Data dir ownership | Data dir must be the **faucet wallet’s** node instance, not an operator personal wallet |
| `genesis.toml` | Must match intended local testnet; optional `--genesis` override if config adds `FAUCET_GENESIS_PATH` |
| `chain.blocks` | Synced enough for correct nonce and balance checks |
| `pending_tx.tril` | Queue file; must be protected by worker + file lock during `node send` |
| Leaked seed | **Never reuse**; rotate wallet and update data dir if compromise suspected |
| Faucet code | Must **not** read `wallet.seed` unless a later milestone explicitly scopes faucet-side seed access |

Operator must fund the faucet wallet with a **limited** testnet TRIL balance before enabling payouts.

---

## 6. DB readiness checklist

Before first real payout (Section 6 — **complete as of MVP 3d-3a** for schema only):

- [x] `schema_migrations` table introduced
- [x] Migration `001` adds payout tracking columns per [faucet_payout_db_design.md](faucet_payout_db_design.md)
- [x] Migration is **idempotent** (safe re-run or version-guarded)
- [x] Startup **fails closed** on migration error (process exits non-zero)
- [x] Tests cover **fresh DB** (create from scratch)
- [x] Tests cover **existing DB upgrade** (dry-run rows preserved)
- [x] **No data loss** for existing `dry_run_accepted` claims
- [x] Indexes created: `idempotency_key`, `status+created_at`, `address+created_at`, `ip+created_at`, partial `tx_hash`

Columns are present but **not yet used** for real payout flow. Worker, row claiming, and CLI execution remain blockers.

---

## 7. Worker readiness checklist

Before first real payout:

- [ ] **One worker** processes **one payout at a time** (in-process mutex)
- [ ] **Conditional row claim** prevents two workers from processing the same row
- [ ] Status transitions: `payout_requested` → `payout_queued` | `payout_failed` | `payout_unknown` | `payout_backend_unavailable` | `payout_misconfigured`
- [ ] **`payout_unknown` blocks retry** until reconciliation
- [ ] **Graceful shutdown** documented: in-flight CLI bounded wait; ambiguous → `payout_unknown`
- [ ] **Startup reconciliation** documented: scan `payout_requested` / `payout_unknown`; inspect `pending_tx.tril`; no blind CLI retry

See [faucet_payout_worker_design.md](faucet_payout_worker_design.md).

---

## 8. CLI execution readiness checklist

When subprocess execution is added (MVP 3d-3+):

| Requirement | Detail |
|-------------|--------|
| No shell | `Command::new(argv[0]).args(&argv[1..])` only — never `sh -c` |
| Args from builder | Use `build_cli_send_args`; separate argv elements |
| Subprocess timeout | Configurable; ambiguous timeout → `payout_unknown` |
| Stdout parser | Match verified success line (see below) |
| Malformed output | Reject; do not invent tx hash |
| Stderr | Sanitize before logging; never return to client |
| Logs | No full secret paths; no seed material |
| Exit codes | Map to `payout_failed` vs `payout_backend_unavailable` vs `payout_unknown` |

### Verified stdout line to parse

```text
Queued tx <TX_HASH> -> <RECEIVER> amount <AMOUNT> fee <FEE> (nonce <NONCE>)
```

Parser must extract `TX_HASH` and validate receiver/amount/fee against request where practical.

**Today:** `CliPayoutAdapter::submit_payout` returns `Disabled`; no subprocess exists.

---

## 9. Local smoke test checklist

Manual test plan for **local development only** after MVP 3d-3 implementation:

1. [ ] Start local testnet with known genesis and data dir
2. [ ] Ensure **`node run`** is running (or start after queue — document operator order)
3. [ ] Verify faucet wallet balance in node state (sufficient for claim + fee)
4. [ ] Start faucet with **`FAUCET_DRY_RUN=true`**; POST claim → `dry_run_accepted`
5. [ ] Verify rate limits still work in dry-run
6. [ ] Review this checklist and [faucet_secret_config_plan.md](faucet_secret_config_plan.md) operator section
7. [ ] Enable payout gates **locally only**: `FAUCET_DRY_RUN=false`, `FAUCET_ENABLE_PAYOUTS=true`, `FAUCET_NETWORK=testnet`, CLI paths set
8. [ ] POST real claim → response `payout_queued` (or `payout_requested` then poll) with `tx_hash`
9. [ ] Verify entry in `pending_tx.tril` and/or stdout log server-side
10. [ ] Verify **`node run`** seals tx into chain
11. [ ] Verify receiver balance updates on testnet
12. [ ] Repeat claim same address → rate limited or blocked by open payout state
13. [ ] Inspect logs: **no seeds, no full secret paths, no raw stderr to clients**
14. [ ] Confirm operator understands **queued is not confirmed** until chain reflects balance

---

## 10. Do-not-start-public checklist

**Public deployment is not allowed** until all of the following:

- [ ] Local payout smoke test (Section 9) passes on a clean environment
- [ ] Abuse controls reviewed (cooldowns, daily caps, IP limits)
- [ ] Reverse proxy trust configured safely (`X-Forwarded-For` not trusted by default)
- [ ] TLS / Cloudflare (or equivalent) reviewed for production hostname
- [ ] Monitoring: claim rate, failure rate, `payout_unknown` count, wallet balance alerts
- [ ] Faucet wallet balance is **capped** and refill procedure documented
- [ ] Operator runbook exists (reconciliation, rotation, incident response)
- [ ] Frontend displays warning: **testnet TRIL has no real-world value**

MVP 3d-5 security/ops review remains the formal gate for any internet-facing deployment.

---

## 11. Decision gate

> **Real CLI execution may only begin after this checklist is reviewed and MVP 3d-3a/3d-3b are complete.**

Suggested split (implementer reference):

| Phase | Scope |
|-------|--------|
| **MVP 3d-3a** | DB migration `001`, `payout_requested` insert path, startup gates for payout mode |
| **MVP 3d-3b** | Payout worker, row claiming, file lock, CLI subprocess + parser (local-only, gated) |

Do not enable `FAUCET_ENABLE_PAYOUTS=true` in shared or production environments until Sections 6–9 are checked off for that environment.

---

## 12. Non-goals

- **No code changes** in MVP 3d-2f
- **No DB migration** in this milestone
- **No command execution**
- **No public deployment**
- **No frontend** work
- **No changes to `trilogicon-core`**
- **No change to `/api/claim`** runtime behavior

---

## 13. Done checklist

- [x] `docs/faucet_local_payout_readiness_checklist.md` exists (this file).
- [x] Root `README.md` links to this document.
- [x] [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md) references this document.
- [x] [faucet_payout_db_design.md](faucet_payout_db_design.md) references this document.
- [x] [faucet_payout_worker_design.md](faucet_payout_worker_design.md) references this document.
- [x] No runtime behavior changed.
- [x] `cargo fmt`, `cargo test`, `cargo clippy -- -D warnings` pass from `faucet/`.

---

## References

- [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md)
- [faucet_payout_status_model.md](faucet_payout_status_model.md)
- [faucet_payout_worker_design.md](faucet_payout_worker_design.md)
- [faucet_payout_db_design.md](faucet_payout_db_design.md)
- [faucet_secret_config_plan.md](faucet_secret_config_plan.md)
