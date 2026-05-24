# Trilogicon testnet tools

Public utilities for the Trilogicon **testnet**: faucet service, future status page, explorer helpers, and operational scripts.

This repository is **not** the Trilogicon core node. Protocol and consensus code live elsewhere; this repo only hosts tooling that operators and users run against the network.

## Faucet backend (dry-run)

The first component is a minimal **Rust + Axum** HTTP service under `faucet/`. It records claim requests in SQLite and enforces per-address and per-client-IP cooldowns. It does **not** connect to a node, invoke the CLI, load a wallet seed, or broadcast transactions. **All payouts are disabled** until real payout logic is added behind explicit configuration.

### Prerequisites

- Rust (stable), e.g. via [rustup](https://rustup.rs/)

### Run

```bash
cd faucet
cargo run
```

By default the server listens on `127.0.0.1:8787` and uses a SQLite file `faucet.db` in the current working directory.

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FAUCET_BIND_ADDR` | `127.0.0.1:8787` | Socket address to bind |
| `FAUCET_DATABASE_URL` | `sqlite:faucet.db` | SQLite URL for sqlx |
| `FAUCET_DRY_RUN` | `true` | When `true`, claims are recorded only; no TRIL is sent. When `false`, the service rejects claims with `payouts_not_enabled` (this build never sends TRIL). |
| `FAUCET_CLAIM_AMOUNT` | `10` | Amount echoed in API responses and stored with each claim |
| `FAUCET_COOLDOWN_SECONDS` | `3600` | Minimum seconds between two claims for the same address |
| `FAUCET_IP_COOLDOWN_SECONDS` | `3600` | Minimum seconds between two claims from the same client IP |

### Client IP tracking

Each claim stores the client IP in the `claims.ip` column. In local/dev mode the service uses the **socket peer address** from Axum `ConnectInfo` (the address of the TCP connection). **`X-Forwarded-For` is not trusted** in this build; adding reverse-proxy support should be an explicit, audited change (e.g. only when behind a known proxy and with a trusted header configuration).

Wallet seeds, CAPTCHA, and node integration are intentionally **not** implemented in this skeleton.

### Secret and payout configuration (MVP 3)

Before real testnet payouts are enabled, read the safety plan: **[docs/faucet_secret_config_plan.md](docs/faucet_secret_config_plan.md)**. It covers threat model, fail-closed startup rules, secret storage, logging boundaries, and phased implementation (3a–3e). **This milestone does not enable payouts or load seeds.**

### Payout-related configuration (MVP 3b, validation only)

MVP 3b keeps future payout-related environment variables in the config layer and adds wallet seed path boundary validation. They are **parsed and validated only**—no TRIL is sent, no seed files are opened, and no CLI or node integration exists.

| Variable | Default | MVP 3b behavior |
|----------|---------|-----------------|
| `FAUCET_ENABLE_PAYOUTS` | `false` | **Must stay `false`.** Setting `true` fails startup (reserved for a later payout milestone). |
| `FAUCET_NETWORK` | `testnet` | Must be `testnet`; any other value fails startup. |
| `FAUCET_WALLET_SEED_PATH` | _(unset)_ | Optional; if set, must be an absolute path outside this repository with no `..` traversal. The file is **not** read. |
| `FAUCET_NODE_MODE` | `disabled` | One of `disabled`, `cli`, `rpc`; no node behavior yet. |
| `FAUCET_NODE_CLI_PATH` | _(unset)_ | Parsed/stored only. |
| `FAUCET_NODE_DATA_DIR` | _(unset)_ | Parsed/stored only. |
| `FAUCET_FIXED_FEE` | `1` | Must be positive. |
| `FAUCET_MAX_DAILY_CLAIMS` | `1000` | Must be positive. |
| `FAUCET_MAX_DAILY_AMOUNT` | `10000` | Must be positive. |

`GET /api/status` exposes safe fields (`payouts_enabled`, `network`, `node_mode`) and **does not** expose seed paths, CLI paths, or node data directories. `FAUCET_DRY_RUN=false` still returns `payouts_not_enabled` on claim requests.

### Payout adapter (MVP 3c, dry-run only)

MVP 3c adds an internal `PayoutAdapter` trait and a `DryRunPayoutAdapter` implementation. Claim handling submits a payout request through this adapter after validation and rate limits, then records the adapter result in SQLite.

- Only the **dry-run adapter** is wired; it never sends TRIL.
- No CLI, RPC, or node integration exists.
- No seed files are read; no keys are derived.
- `FAUCET_ENABLE_PAYOUTS=true` still fails startup.
- Real testnet payouts remain a later milestone (MVP 3d).

### CLI payout adapter skeleton (MVP 3d-1)

MVP 3d-1 adds a `CliPayoutAdapter` skeleton and pure `build_cli_send_args` helper for future `node send`-style invocations.

- **No CLI commands are executed** — no `Command`, spawn, or subprocess.
- `CliPayoutAdapter::submit_payout` always returns `Disabled`.
- **`default_payout_adapter()` still returns `DryRunPayoutAdapter` only** for live requests.
- No seed files are read; no TRIL is sent.
- Real CLI payout execution remains MVP 3d-3 or later.

### CLI argv construction (MVP 3d-2 / 3d-2b)

MVP 3d-2 hardens `build_cli_send_args` with stricter validation (empty paths, zero amount/fee, control characters, non-dry-run requests rejected for argv building).

MVP 3d-2b aligns the argv builder with the **verified** Trilogicon core CLI shape:

```text
<cli_path> send --data-dir <node_data_dir> [--genesis <genesis_path>] <receiver> <amount> <fee>
```

- **`--genesis` is optional** — core defaults to `{data-dir}/genesis.toml` when omitted.
- Fee is included explicitly even though core defaults fee to `1`.
- **Still no command execution** — no `Command`, spawn, or subprocess.
- Core `node send` **queues** the transaction to `pending_tx.tril`; it does not broadcast directly. A running `node run` must drain the queue and seal blocks for payout completion.
- Live claims still use `DryRunPayoutAdapter` only.
- Real CLI payout execution remains MVP 3d-3 or later.

### Payout status model (MVP 3d-2c)

Before implementing real payout execution, read: **[docs/faucet_payout_status_model.md](docs/faucet_payout_status_model.md)**. It defines claim/payout status vocabulary (`dry_run_accepted`, `payout_queued`, `payout_confirmed`, …), queue vs confirmation semantics, DB column recommendations, duplicate-payout mitigations, and API response shapes. **This milestone does not change runtime behavior or DB schema.**

### Local testnet payout design (MVP 3d-prep)

Before implementing real testnet sends, read: **[docs/faucet_local_testnet_payout_design.md](docs/faucet_local_testnet_payout_design.md)**. It describes the intended CLI vs RPC adapter path, fail-closed config, failure statuses, idempotency risks, and phased MVP 3d rollout. **This milestone does not enable payouts or change `/api/claim` behavior.**

### Example `curl` commands

Health:

```bash
curl -sS http://127.0.0.1:8787/api/health | jq
```

Status:

```bash
curl -sS http://127.0.0.1:8787/api/status | jq
```

Claim (dry-run; body size is capped at 512 bytes):

```bash
curl -sS -X POST http://127.0.0.1:8787/api/claim \
  -H 'Content-Type: application/json' \
  -d '{"address":"tl1test_example"}' | jq
```

### Warning

The current faucet is **dry-run only**. It does not mint coins, sign transactions, or send TRIL. Per-IP rate limits reduce abuse but do not enable payouts. Treat any `claim_amount` or future `tx_hash` fields as placeholders until payout integration is completed and audited.

### Development

```bash
cd faucet
cargo fmt
cargo test
cargo clippy -- -D warnings
```
