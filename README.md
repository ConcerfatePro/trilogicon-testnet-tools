# Trilogicon testnet tools

Public utilities for the Trilogicon **testnet**: faucet service, future status page, explorer helpers, and operational scripts.

This repository is **not** the Trilogicon core node. Protocol and consensus code live elsewhere; this repo only hosts tooling that operators and users run against the network.

## Faucet backend (dry-run)

The first component is a minimal **Rust + Axum** HTTP service under `faucet/`. It records claim requests in SQLite and enforces a per-address cooldown. It does **not** connect to a node, invoke the CLI, load a wallet seed, or broadcast transactions. **All payouts are disabled** until real payout logic is added behind explicit configuration.

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

Wallet seeds, CAPTCHA, and node integration are intentionally **not** implemented in this skeleton.

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

The current faucet is **dry-run only**. It does not mint coins, sign transactions, or send TRIL. Treat any `claim_amount` or future `tx_hash` fields as placeholders until payout integration is completed and audited.

### Development

```bash
cd faucet
cargo fmt
cargo test
cargo clippy -- -D warnings
```
