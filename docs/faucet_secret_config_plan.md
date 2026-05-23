# Faucet secret and payout configuration safety plan

## 1. Purpose

This document defines how the Trilogicon testnet faucet must handle **secrets**, **payout configuration**, and **operator workflow** before any real testnet TRIL transaction-sending code is added.

It is a **documentation-first** milestone (Faucet MVP 3). The goal is to agree on fail-closed rules, environment variables, secret storage, logging boundaries, and phased implementation so that enabling payouts later is deliberate, auditable, and reversible—not an accidental config flip.

Nothing in this plan authorizes real payouts by itself. Implementation of payout paths remains gated behind explicit future milestones and code review.

---

## 2. Threat model

| Threat | Description | Mitigations (planned) |
|--------|-------------|------------------------|
| **Accidental seed commit** | Operator copies `wallet.seed` into the repo or commits a backup. | `.gitignore` for seeds and `.env`; startup refuses seed paths inside the git tree; pre-commit discipline; never log seed material. |
| **Leaked `.env`** | Environment file with secrets pushed to git, pasted in chat, or shared in CI artifacts. | Ignore `.env`; load secrets from env or out-of-repo files only; document that `.env` is local-only. |
| **Logs exposing secrets** | Structured or panic logs print seed, private key, or full secret file paths. | Logging rules (Section 7); redact paths; audit log statements before payout MVP. |
| **Misconfigured dry-run mode** | `FAUCET_DRY_RUN=false` without other gates, or contradictory flags, enables unintended behavior. | Dual gates: `FAUCET_DRY_RUN` + `FAUCET_ENABLE_PAYOUTS`; startup fail-closed on contradictions (Section 9). |
| **Faucet wallet drained by abuse** | Bots or scripted claims exhaust testnet TRIL. | Per-address and per-IP cooldowns (existing); future daily caps; fixed claim amount; monitor balance. |
| **Operator points at mainnet** | If a mainnet ever exists, wrong network ID or node data dir sends real value. | Explicit `FAUCET_NETWORK=testnet` required for payouts; refuse non-testnet; **no mainnet** in scope (Section 12). |
| **Unsafe reverse proxy** | Trusting `X-Forwarded-For` from the open internet lets attackers spoof IPs and bypass IP limits. | Today: TCP peer IP only; future proxy support must be explicit, allowlisted, and documented. |
| **Database leak** | `faucet.db` copied or exposed; reveals addresses, IPs, claim history. | Ignore DB files in git; restrict file permissions on server; treat DB as sensitive operations data. |
| **Server compromise** | Attacker with shell access reads env, seed file, or process memory. | Out-of-repo secrets, minimal balance, rotation procedure, OS hardening (MVP 3e). |

---

## 3. Current state (as of MVP 2)

The faucet backend under `faucet/` is **dry-run only**:

| Area | Status |
|------|--------|
| Payouts | **None.** No transaction build, sign, or broadcast. |
| Wallet seed | **Not loaded.** No code reads `FAUCET_WALLET_SEED_PATH` yet. |
| Trilogicon CLI / node | **Not called.** No subprocess or RPC client. |
| `trilogicon-core` | **Not modified** by this repo. |
| Claims | SQLite `claims` table: address, IP, amount, dry_run, status, `created_at`. |
| Rate limits | Per-address (`FAUCET_COOLDOWN_SECONDS`) and per-IP (`FAUCET_IP_COOLDOWN_SECONDS`). |
| Client IP | `ConnectInfo<SocketAddr>` → `peer.ip()` stored as string; **no port**. |
| Proxy headers | **`X-Forwarded-For` not trusted.** |
| `FAUCET_DRY_RUN=false` | Returns `payouts_not_enabled` (503) before DB insert; still no TRIL sent. |

Default configuration keeps the service safe: `FAUCET_DRY_RUN` defaults to `true`.

---

## 4. Required payout safety gates

Before **any** real testnet TRIL payout is enabled in production, **all** of the following must be satisfied:

1. **Explicit dry-run off:** `FAUCET_DRY_RUN=false` (alone is insufficient).
2. **Explicit payout enable:** `FAUCET_ENABLE_PAYOUTS=true` (new; not implemented yet).
3. **Explicit testnet network:** `FAUCET_NETWORK=testnet` (or equivalent canonical testnet identifier enforced at startup).
4. **Faucet wallet secret** loaded from environment or a **local secret file outside the git repository** (never committed).
5. **Startup refusal** if the resolved seed path lies inside the repository working tree / git root.
6. **Startup refusal** if any required payout configuration is missing or contradictory (Section 9).
7. **Limited faucet balance** — operator funds a dedicated testnet wallet with a capped amount; alerts when low.
8. **Fixed claim amount** — positive, configured maximum per claim (existing `FAUCET_CLAIM_AMOUNT` pattern).
9. **Rate limits enabled** — non-zero address and IP cooldowns (and future daily caps).
10. **Operator confirmation checklist completed** (Section 10) and recorded (ticket/runbook sign-off).

Payout code must not ship until this document’s gates are implemented and reviewed.

---

## 5. Secret loading options

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| **A. Environment variable** | e.g. `FAUCET_WALLET_SEED` or path-only `FAUCET_WALLET_SEED_PATH` with contents read at startup. | Simple for containers and systemd; no extra file on disk in app dir. | Visible in `/proc`, process listings, crash dumps, some CI logs; easy to leak via `docker inspect`. |
| **B. Local secret file outside repo** | e.g. `/etc/trilogicon/faucet/wallet.seed` or `~/.config/trilogicon-faucet/wallet.seed`, referenced by `FAUCET_WALLET_SEED_PATH`. | Clear separation from git; familiar for operators; path can be locked down with permissions. | File permissions and backup policy are operator responsibility; path must stay outside repo. |
| **C. OS secret manager (later)** | systemd credentials, HashiCorp Vault, cloud KMS, etc. | Best long-term for production; audit and rotation tooling. | Higher setup cost; not required for first payout MVP. |

**Recommendation for first real payout MVP (3d):** **B (file outside repo)** or **A (environment variable)** with **path-based loading preferred** over embedding raw seed in env when possible. Read the file once at startup, zeroize buffers where feasible, and never log contents. Plan **C** for MVP 3e deployment hardening.

---

## 6. Required `.gitignore` behavior

The faucet must never commit secrets, local env, or SQLite WAL files. The following patterns are required (and are present in `faucet/.gitignore`):

| Pattern | Purpose |
|---------|---------|
| `.env` | Local environment overrides; may reference secret paths. |
| `wallet.seed` | Default faucet seed filename. |
| `*.seed` | Any seed backup variant. |
| `faucet.db` | Default SQLite database. |
| `*.db` | Other SQLite database paths. |
| `*.db-wal` | SQLite write-ahead log. |
| `*.db-shm` | SQLite shared memory. |

Repository root may add a root `.gitignore` later if multiple tools share the repo; faucet-local ignore remains the minimum bar.

---

## 7. Logging rules

Logging must be **useful for operations** without becoming a **secret channel**.

**Never log:**

- Wallet seed (mnemonic, hex seed, or derived material).
- Private keys or signing payloads.
- Full contents of secret files.
- Full secret file paths when a short basename or “configured” is enough (prefer redaction).

**Safe to log:**

- Faucet **public** address (derived at startup for verification).
- Claim amount, dry-run vs payout mode flags (boolean only, not seed).
- Transaction hash after broadcast (testnet only).
- Rate-limit decisions (`rate_limited`, `ip_rate_limited`) without storing extra PII beyond what the API already exposes.
- Generic errors (`database_error`, `payouts_not_enabled`) without SQL strings or internal paths in client responses; server logs may include more detail at `debug`/`error` tiers but still not secrets.

Use structured logging levels: `info` for claim outcomes, `warn` for limit hits, `error` for failures—audit new log lines in payout PRs.

---

## 8. Configuration proposal and parsed fields

Future environment variables for payout eras. MVP 3a parses these values with fail-safe defaults. MVP 3b validates the wallet seed path string boundary only. Neither milestone reads seed contents, opens seed files, sends TRIL, or connects to a node.

| Variable | Default (proposed) | Role |
|----------|------------------|------|
| `FAUCET_ENABLE_PAYOUTS` | `false` | Master switch for real sends; must pair with `FAUCET_DRY_RUN=false`. |
| `FAUCET_NETWORK` | `testnet` | Canonical network id; startup fails if not testnet when payouts enabled. |
| `FAUCET_WALLET_SEED_PATH` | _(unset)_ | Optional until payouts exist. If set in MVP 3b, it must be an absolute path **outside repo** with no `..` traversal; the file is not read. Required only when future payouts are enabled. |
| `FAUCET_NODE_MODE` | _(unset)_ | `cli` or `rpc` — how to reach Trilogicon node (future). |
| `FAUCET_NODE_CLI_PATH` | _(unset)_ | Path to Trilogicon CLI binary when `NODE_MODE=cli`. |
| `FAUCET_NODE_DATA_DIR` | _(unset)_ | Node data directory for CLI/RPC context. |
| `FAUCET_FIXED_FEE` | _(TBD)_ | Fee paid per faucet transaction (testnet). |
| `FAUCET_MAX_DAILY_CLAIMS` | _(TBD)_ | Global cap on claim count per 24h. |
| `FAUCET_MAX_DAILY_AMOUNT` | _(TBD)_ | Global cap on TRIL dispensed per 24h. |

**Existing variables (today):** `FAUCET_BIND_ADDR`, `FAUCET_DATABASE_URL`, `FAUCET_DRY_RUN`, `FAUCET_CLAIM_AMOUNT`, `FAUCET_COOLDOWN_SECONDS`, `FAUCET_IP_COOLDOWN_SECONDS`.

---

## 9. Startup fail-closed rules

When payout mode is requested, the process **must exit non-zero** at startup if any rule below fails. (Rules apply to future implementation; not enforced in MVP 3 code.)

| Condition | Action |
|-----------|--------|
| `FAUCET_DRY_RUN=true` and `FAUCET_ENABLE_PAYOUTS=true` | Refuse start (contradiction). |
| `FAUCET_DRY_RUN=false` and `FAUCET_ENABLE_PAYOUTS=false` | Refuse start (would imply “live” without enable switch). |
| `FAUCET_ENABLE_PAYOUTS=true` and `FAUCET_NETWORK` ≠ `testnet` | Refuse start. |
| Payouts enabled and seed missing / unreadable | Refuse start. |
| Resolved seed path inside git repository root | Refuse start. MVP 3b enforces this lexically without reading or canonicalizing the file. |
| Address or IP cooldown seconds are `0` while payouts enabled | Refuse start (limits disabled). |
| `FAUCET_CLAIM_AMOUNT` ≤ 0 | Refuse start. |
| Database unavailable | Refuse start. |
| `FAUCET_NODE_MODE` set but CLI/RPC path or data dir missing | Refuse start. |

**Dry-run default path (today):** Only `FAUCET_DRY_RUN` matters; payouts remain disabled with `payouts_not_enabled` if dry-run is false.

---

## 10. Operator checklist

Use this checklist before enabling real testnet payouts (after MVP 3d implementation):

1. [ ] Create a **dedicated faucet testnet wallet** (not a personal or mainnet wallet).
2. [ ] Fund it with a **limited** amount of testnet TRIL.
3. [ ] Store the seed in a file **outside** this repository; set restrictive permissions (e.g. `0600`).
4. [ ] Configure environment variables per Section 8; verify `.env` is not committed.
5. [ ] Run the faucet in **dry-run** mode against the same DB and config; verify claims insert and rate limits.
6. [ ] Verify per-address and per-IP rate limits with test clients (including same IP, different addresses).
7. [ ] Review [README](../README.md) and this plan with a second operator.
8. [ ] Only then set `FAUCET_DRY_RUN=false` and `FAUCET_ENABLE_PAYOUTS=true` with `FAUCET_NETWORK=testnet`.
9. [ ] Monitor faucet balance, claim rate, and logs; alert on low balance.
10. [ ] If seed or env leaks: **rotate** seed (new wallet), drain/retire old wallet, update secret path, audit logs and DB access.

---

## 11. Future implementation phases

| Phase | Scope | Payouts | Seed read |
|-------|--------|---------|-----------|
| **MVP 3 (this doc)** | Secret/config safety plan, README link | No | No |
| **MVP 3a** | Config types and parsing for future vars; defaults fail-safe (**implemented**) | No | No |
| **MVP 3b** | Seed path boundary validation; require absolute out-of-repo paths; still no payout (**implemented**) | No | No (validate path only; do not open file) |
| **MVP 3c** | `PayoutAdapter` trait + `DryRunPayoutAdapter` only; claim flow uses adapter shape; no real send (**implemented**) | No real send | No seed read |
| **MVP 3d-prep** | Local testnet payout design note ([faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md)); no execution (**implemented**) | No | No |
| **MVP 3d** | Real testnet payout behind all gates in Section 4 and the [local payout design note](faucet_local_testnet_payout_design.md) (3d-1…3d-5) | Yes (testnet only) | Yes |
| **MVP 3e** | Deployment hardening: reverse proxy, TLS, OS secrets, monitoring | Yes | Yes |

Each phase requires tests and clippy clean; prior dry-run behavior remains until 3d explicitly enables sends.

---

## 12. Non-goals

- **No mainnet** support or configuration.
- **No real payouts** in MVP 3 (documentation only).
- **No seed loading** before MVP 3d.
- **No Trilogicon node or CLI integration** in MVP 3.
- **No frontend** work.
- **No CAPTCHA** unless separately scoped.
- **No changes to `trilogicon-core`.**

---

## 13. Done checklist (MVP 3 complete)

- [x] `docs/faucet_secret_config_plan.md` exists (this file).
- [x] Root `README.md` links to this plan.
- [x] No payout code path added in `faucet/src`.
- [x] `cargo fmt`, `cargo test`, `cargo clippy -- -D warnings` pass from `faucet/`.
- [x] Faucet remains dry-run by default and safe without operator action.

---

## References

- Faucet backend: `faucet/`
- Ignore rules: `faucet/.gitignore`
- Current API: `GET /api/health`, `GET /api/status`, `POST /api/claim`
- Local testnet payout design (MVP 3d-prep): [faucet_local_testnet_payout_design.md](faucet_local_testnet_payout_design.md)
