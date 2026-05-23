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
