# Local testnet payout design note (MVP 3d-prep)

## 1. Purpose

This document defines **how real testnet TRIL payouts should eventually be implemented** in the Trilogicon faucet—safely, audibly, and without shortcuts—**before any payout execution code is written**.

It is a **documentation-only** milestone (MVP 3d-prep). Reading or following this note does **not** enable payouts. Implementation remains gated behind explicit future milestones (MVP 3d-1 through 3d-5) and operator review.

Related documents:

- [faucet_secret_config_plan.md](faucet_secret_config_plan.md) — threat model, secrets, fail-closed startup rules
- Faucet backend: `faucet/` (Rust + Axum, dry-run today)

---

## 2. Current state

As of MVP 3c, the faucet backend has:

| Area | Status |
|------|--------|
| HTTP API | `GET /api/health`, `GET /api/status`, `POST /api/claim` |
| Mode | **Dry-run only** by default (`FAUCET_DRY_RUN=true`) |
| Payout adapter | `PayoutAdapter` trait exists; only `DryRunPayoutAdapter` is wired |
| Adapter result | `tx_hash: null`, `status: dry_run_accepted`, `dry_run: true` |
| Config gates | Payout-related env vars parsed; `FAUCET_ENABLE_PAYOUTS=true` **fails startup** |
| Seed path | Boundary validation (absolute, outside repo); **file never opened** |
| Rate limits | Per-address and per-IP cooldowns |
| Real payouts | **Disabled** — no seed read, no CLI/node/RPC calls, no TRIL sent |

`FAUCET_DRY_RUN=false` returns `payouts_not_enabled` (503) before any adapter work that could imply live sends.

---

## 3. Payout model

When real payouts are enabled (future MVP 3d), the faucet must send **testnet TRIL from a normal funded faucet wallet/account** using the same rules as any other participant.

**The faucet must not:**

- Mint coins or create value outside consensus rules
- Bypass signatures or use unsigned sends
- Bypass account nonces / ordering rules
- Bypass fee requirements
- Use special-case protocol shortcuts unavailable to ordinary wallets

**The faucet must:**

- Build, sign, and broadcast (or delegate signing to the CLI/node) **normal Trilogicon transactions**
- Respect fixed `FAUCET_CLAIM_AMOUNT` and `FAUCET_FIXED_FEE` configured at startup
- Operate only on `FAUCET_NETWORK=testnet`

The faucet is a **limited hot wallet**, not a protocol authority.

---

## 4. Adapter options

Future `PayoutAdapter` implementations can talk to Trilogicon in several ways:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| **A. CLI adapter** | Faucet spawns Trilogicon node CLI subprocess (e.g. `node send …`) | Reuses existing node send behavior; likely fastest path for **local testnet** smoke tests | Subprocess lifecycle, stdout/stderr parsing, timeouts, version drift, ops fragility |
| **B. RPC adapter** | Faucet calls a stable node HTTP/RPC API | Cleaner for long-running public services; easier observability and retries | Requires a **stable node RPC** that may not exist yet; more design work upfront |
| **C. Direct library adapter** | Faucet links shared core/signing libraries | Potentially efficient; no subprocess | Tight coupling to `trilogicon-core` internals; higher audit burden; **out of scope** for this repo’s first payout |

**Recommendation:**

- **First real local testnet payout (MVP 3d-3/3d-4):** **CLI adapter** only if command behavior is stable, heavily gated, and subprocess boundaries are hardened (no shell, timeouts, sanitized errors).
- **Later public / production faucet:** **RPC adapter** once node RPC is stable and documented—prefer over CLI for operability.

Do **not** implement direct core library integration in `trilogicon-testnet-tools` without an explicit architectural decision and security review.

---

## 5. Proposed future CLI payout flow

Future claim path when all payout gates are satisfied (design only—not implemented):

```
POST /api/claim
  → validate address
  → check address + IP rate limits
  → verify runtime payout gates (dry_run off, enable_payouts on, network testnet, …)
  → construct PayoutRequest { address, amount, fee, dry_run: false }
  → CliPayoutAdapter::submit_payout
       → invoke CLI send with fixed args (no shell interpolation)
       → parse tx hash from stdout ("Queued tx …")
       → record **queued** status (tx in pending_tx.tril; not yet sealed)
  → insert claim row { address, ip, amount, dry_run: false, status, tx reference }
  → return JSON { ok, dry_run: false, tx_hash, … }
```

**Note:** `node send` queues to `pending_tx.tril`; `node run` must drain and seal. Payout completion is not immediate at CLI success.

If any gate fails at **startup**, the process must not start. If a gate fails at **request time** (e.g. backend unavailable), return a stable API error and record failure status without claiming success.

---

## 6. Verified CLI command shape

**Verified against `trilogicon-core` (MVP 3d-2b).** Binary: `node`.

```bash
<FAUCET_NODE_CLI_PATH> send \
  [--data-dir <FAUCET_NODE_DATA_DIR>] \
  [--genesis <GENESIS_PATH>] \
  <RECEIVER> \
  <AMOUNT> \
  [<FEE>]
```

| Item | Verified behavior |
|------|-------------------|
| `--data-dir` | Optional; defaults to `.` |
| `--genesis` | Optional; defaults to `{data-dir}/genesis.toml` when omitted |
| Positional order | `RECEIVER`, `AMOUNT`, `FEE` |
| `AMOUNT` / `FEE` | u64 decimals; `FEE` defaults to `1` if omitted |
| Wallet / seed | CLI loads `{data-dir}/wallet.seed` (faucet must not read seed until a later MVP) |
| Chain state | Reads genesis + local `chain.blocks`; computes nonce from committed state |
| Send side effect | Signs tx and **appends to `pending_tx.tril`** — does **not** broadcast directly to a running node |
| Completion | `node run` must later drain the queue and seal blocks |

Success stdout (for future parsing in MVP 3d-3+):

```text
Queued tx <TX_HASH> -> <RECEIVER> amount <AMOUNT> fee <FEE> (nonce <NONCE>)
```

The faucet argv builder (`build_cli_send_args`) constructs separate argv elements matching this shape. It always includes `--data-dir`, receiver, amount, and fee explicitly; `--genesis` is included only when configured. **No execution** until MVP 3d-3.

**Payout success semantics (future):** A CLI exit with the success line above means the transaction is **queued**, not confirmed or sealed. Claim status and API responses must distinguish `payout_queued` (or equivalent) from `payout_confirmed` / sealed-on-chain states. Do not return `ok: true` with a final `tx_hash` until the operator-defined confirmation policy is met (likely after `node run` has sealed the block).

Open questions for MVP 3d-3:

| Item | Notes |
|------|--------|
| Exit codes | Map non-zero exit to `PayoutError::Rejected` vs `BackendUnavailable` |
| Queue drain | How faucet observes sealed state vs queued-only (polling, operator workflow) |

---

## 7. Required config before real payouts

All of the following must be set and validated at startup when payouts are enabled (future behavior):

| Variable | Required value / notes |
|----------|------------------------|
| `FAUCET_DRY_RUN` | `false` |
| `FAUCET_ENABLE_PAYOUTS` | `true` |
| `FAUCET_NETWORK` | `testnet` |
| `FAUCET_WALLET_SEED_PATH` | Absolute path **outside repo**; file must exist and be readable only when 3d enables read |
| `FAUCET_NODE_MODE` | `cli` (for CLI adapter) |
| `FAUCET_NODE_CLI_PATH` | Absolute path to Trilogicon CLI binary |
| `FAUCET_NODE_DATA_DIR` | Node data directory for testnet instance |
| `FAUCET_FIXED_FEE` | Positive integer |
| `FAUCET_MAX_DAILY_CLAIMS` | Positive integer |
| `FAUCET_MAX_DAILY_AMOUNT` | Positive integer |
| `FAUCET_CLAIM_AMOUNT` | Positive integer |
| `FAUCET_COOLDOWN_SECONDS` | **Non-zero** when payouts enabled |
| `FAUCET_IP_COOLDOWN_SECONDS` | **Non-zero** when payouts enabled |

**Additional testnet config (TBD):**

- Genesis file path or network profile identifier aligned with local testnet
- Faucet wallet funded with a **capped** testnet balance before enabling payouts
- Optional: `FAUCET_GENESIS_PATH` — only if overriding core default `{data-dir}/genesis.toml`

Today, `FAUCET_ENABLE_PAYOUTS=true` fails startup in MVP 3a–3c; these combinations are design targets for MVP 3d.

---

## 8. Fail-closed rules

Real payout mode must **refuse to start** if any of the following hold:

| Condition | Action |
|-----------|--------|
| `FAUCET_NETWORK` ≠ `testnet` | Exit non-zero |
| `FAUCET_DRY_RUN=true` and `FAUCET_ENABLE_PAYOUTS=true` | Contradiction — exit |
| `FAUCET_DRY_RUN=false` and `FAUCET_ENABLE_PAYOUTS=false` | Live without enable switch — exit |
| Seed path missing or unreadable | Exit |
| Seed path inside git repository | Exit |
| `FAUCET_NODE_MODE=disabled` | Exit when payouts enabled |
| `FAUCET_NODE_MODE=cli` and CLI path missing | Exit |
| Node data dir missing when required | Exit |
| `FAUCET_FIXED_FEE` ≤ 0 | Exit |
| `FAUCET_CLAIM_AMOUNT` ≤ 0 | Exit |
| Address or IP cooldown = 0 | Exit (limits disabled) |
| Database unavailable | Exit |
| Faucet wallet balance check fails | Exit (if balance check implemented in 3d) |

Until MVP 3d implements these checks together with `FAUCET_ENABLE_PAYOUTS=true`, the faucet remains dry-run only.

---

## 9. Failure handling

Future claim rows and API responses must **not pretend success** when a real send fails.

Proposed **claim `status` values**:

| Status | Meaning |
|--------|---------|
| `dry_run_accepted` | Dry-run mode; no chain send (current behavior) |
| `payout_submitted` | CLI/RPC accepted send; tx hash known (**queued** for CLI path — in `pending_tx.tril`) |
| `payout_confirmed` | Tx sealed on chain (after `node run` or RPC confirmation policy) |
| `payout_rejected` | Node/CLI rejected tx (invalid address, insufficient funds, etc.) |
| `payout_backend_unavailable` | CLI/RPC timeout, process crash, unreachable node |
| `payout_misconfigured` | Adapter misconfiguration detected at request time |
| `payout_failed` | Send attempted; definitive failure |
| `payout_unknown` | Ambiguous outcome (e.g. subprocess lost after possible submit) |

**API error mapping (stable strings, no secret leakage):**

- User-facing errors stay generic (`database_error`, `payouts_not_enabled`, rate limits, `invalid_address`)
- Do not return raw CLI stderr to clients; log sanitized details server-side only

**HTTP success vs failure:**

- Return `ok: true` with `tx_hash` only when status is `payout_submitted` (or equivalent confirmed state)
- Failed real payouts: `ok: false` with appropriate error code; DB row records failure status

---

## 10. Idempotency / duplicate handling

Real payouts introduce **double-spend and duplicate-claim** risks not present in dry-run.

| Risk | Notes |
|------|--------|
| User retry after HTTP timeout | Client may POST again; cooldowns help but do not cover “same second” edge cases |
| DB insert before vs after send | Insert-before-send leaks pending rows; send-before-insert loses audit trail if DB fails |
| CLI succeeds, HTTP response lost | User retries → potential **double payout** unless idempotency or pending state |
| Concurrent requests | Same address/IP may pass cooldown checks under race (known MVP 2 limitation) |

**Design direction (not fully solved in this doc):**

- Consider `pending` status + idempotency key (address + time window or client token) in MVP 3d-4+
- Prefer **single-writer** or DB transaction around “reserve → send → finalize” when payouts enabled
- Document ambiguous `payout_unknown` and operator reconciliation procedure
- Keep per-address and per-IP cooldowns; add daily caps (`FAUCET_MAX_DAILY_*`) at send time

Do not enable public payouts until duplicate/double-payout mitigations are reviewed.

---

## 11. Security considerations

- **Never log** seed, mnemonic, private keys, or raw signing material
- **Never echo** full secret paths in API responses or client-visible errors
- **Sanitize CLI stdout/stderr** before logging; cap log line length
- **Subprocess:** use explicit argv (`Command::new(path).arg(...)`); **no shell** (`sh -c`)
- **Cap subprocess runtime** (timeout → `payout_backend_unavailable`)
- **Fixed amount and fee** from config only—ignore user-supplied amounts in POST body beyond validation
- **Limited faucet wallet** balance; alert when low
- **Rotate wallet** if seed or host compromise suspected
- **Reverse proxy:** do not trust `X-Forwarded-For` unless explicitly configured (see MVP 3e / proxy design)
- **Rate limits** must remain non-zero in payout mode
- **Monitor** claim rate, failure rate, and wallet balance

---

## 12. Local testnet verification plan

Safe manual checklist for future MVP 3d-4 (local only):

1. Run a **local Trilogicon testnet node** with known genesis/data dir.
2. Create a **dedicated faucet wallet**; store seed outside this repo.
3. Fund faucet wallet with a **small** testnet TRIL amount.
4. Start faucet with **`FAUCET_DRY_RUN=true`**; verify claims insert and rate limits work.
5. Review [faucet_secret_config_plan.md](faucet_secret_config_plan.md) and this document.
6. Enable payout mode only with explicit env gates (`FAUCET_DRY_RUN=false`, `FAUCET_ENABLE_PAYOUTS=true`, `FAUCET_NETWORK=testnet`, CLI paths set).
7. POST claim to a **test receiver** address; verify `tx_hash` in response.
8. Verify transaction appears in node chain state / explorer.
9. Repeat claim same address → **rate_limited**; same IP different address → **ip_rate_limited** (within cooldown).
10. POST claim with invalid receiver → **failure recorded**, no false success.
11. Inspect logs: **no seeds, no full secret paths, no raw key material**.

Do not expose payout-enabled faucet to the public internet until MVP 3d-5 review completes.

---

## 13. Implementation phases

| Phase | Scope | Executes CLI? | Sends TRIL? |
|-------|--------|---------------|-------------|
| **MVP 3d-prep (this doc)** | Design note, README link | No | No |
| **MVP 3d-1** | `CliPayoutAdapter` skeleton + `build_cli_send_args`; `submit_payout` returns `Disabled`; no subprocess (**implemented**) | No | No |
| **MVP 3d-2** | CLI argv hardening + extended validation tests; no subprocess (**implemented**) | No | No |
| **MVP 3d-2b** | Align argv builder with verified core `node send` syntax; optional `--genesis`; document queue vs sealed semantics; no subprocess (**implemented**) | No | No |
| **MVP 3d-3** | Local-only execution behind explicit env gates + operator checklist | Yes (local) | Yes (testnet) |
| **MVP 3d-4** | Local testnet payout smoke test + failure-mode tests | Yes (local) | Yes (testnet) |
| **MVP 3d-5** | Security/ops review before any public deployment | — | — |

Each phase requires tests and `cargo clippy -- -D warnings` clean from `faucet/`.

---

## 14. Non-goals

- **No mainnet** support
- **No production public faucet** in MVP 3d-prep or early 3d sub-phases
- **No real payout code** in this milestone
- **No RPC adapter** implementation yet (design only)
- **No direct `trilogicon-core` library integration** in the faucet crate
- **No frontend** work
- **No CAPTCHA** unless separately scoped
- **No changes to `trilogicon-core`** as part of this documentation milestone

---

## 15. Done checklist (MVP 3d-prep complete)

- [x] `docs/faucet_local_testnet_payout_design.md` exists (this file).
- [x] Root `README.md` links to this design note.
- [x] `docs/faucet_secret_config_plan.md` references this note under MVP 3d.
- [x] No payout execution code added in `faucet/src`.
- [x] `/api/claim` dry-run behavior unchanged.
- [x] `cargo fmt`, `cargo test`, `cargo clippy -- -D warnings` pass from `faucet/`.

---

## References

- [faucet_secret_config_plan.md](faucet_secret_config_plan.md)
- Faucet payout module: `faucet/src/payout.rs` (`PayoutAdapter`, `DryRunPayoutAdapter`)
- Config: `faucet/src/config.rs`
