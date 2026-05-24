//! Payout adapter boundary for future CLI/RPC integration (MVP 3c+: dry-run live; CLI skeleton in 3d-1).

use async_trait::async_trait;
use std::sync::Arc;

/// Claim/payout status strings for `claims.status` and API responses.
///
/// Design and lifecycle: `docs/faucet_payout_status_model.md`
#[allow(dead_code)] // referenced when real payouts are wired (MVP 3d-3+)
pub mod claim_status {
    /// Dry-run claim accepted; no transaction sent (current default).
    pub const DRY_RUN_ACCEPTED: &str = "dry_run_accepted";
    /// Real claim recorded before CLI execution attempt.
    pub const PAYOUT_REQUESTED: &str = "payout_requested";
    /// CLI queued tx into `pending_tx.tril`; hash known, not sealed.
    pub const PAYOUT_QUEUED: &str = "payout_queued";
    /// Request rejected before CLI submission.
    pub const PAYOUT_REJECTED: &str = "payout_rejected";
    /// CLI/node backend unavailable.
    pub const PAYOUT_BACKEND_UNAVAILABLE: &str = "payout_backend_unavailable";
    /// Payout configuration invalid.
    pub const PAYOUT_MISCONFIGURED: &str = "payout_misconfigured";
    /// CLI failed clearly; no tx hash produced.
    pub const PAYOUT_FAILED: &str = "payout_failed";
    /// Ambiguous outcome; do not retry blindly.
    pub const PAYOUT_UNKNOWN: &str = "payout_unknown";
    /// Tx sealed in a block (future confirmation worker).
    pub const PAYOUT_CONFIRMED: &str = "payout_confirmed";
}

/// Input for a single faucet payout attempt.
pub struct PayoutRequest {
    pub address: String,
    pub amount: u64,
    pub fee: u64,
    pub dry_run: bool,
}

#[derive(Debug)]
pub struct PayoutResult {
    pub tx_hash: Option<String>,
    pub status: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // returned by future CLI/RPC adapters (MVP 3d-3+)
pub enum PayoutError {
    Disabled,
    Misconfigured,
    InvalidRequest,
    BackendUnavailable,
    Rejected,
    Internal,
}

#[async_trait]
pub trait PayoutAdapter: Send + Sync {
    async fn submit_payout(&self, request: PayoutRequest) -> Result<PayoutResult, PayoutError>;
}

/// Safe parsed fields for a future CLI payout backend (no execution in MVP 3d-1+).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // wired when CLI execution is enabled (MVP 3d-3+)
pub struct CliPayoutConfig {
    pub cli_path: Option<String>,
    pub node_data_dir: Option<String>,
    /// Optional; core defaults to `{node_data_dir}/genesis.toml` when omitted.
    pub genesis_path: Option<String>,
    pub fixed_fee: u64,
}

/// Future CLI adapter skeleton; always disabled until a later MVP 3d sub-phase.
#[allow(dead_code)] // constructed when CLI execution is enabled (MVP 3d-3+)
pub struct CliPayoutAdapter {
    #[allow(dead_code)] // used when CLI execution is enabled in MVP 3d-3+
    config: CliPayoutConfig,
}

impl CliPayoutAdapter {
    #[allow(dead_code)]
    pub fn new(config: CliPayoutConfig) -> Self {
        Self { config }
    }
}

fn contains_control_chars(value: &str) -> bool {
    value.chars().any(|c| c.is_control())
}

fn validate_config_path(value: Option<&str>) -> Result<String, PayoutError> {
    let trimmed = value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(PayoutError::Misconfigured)?;
    if contains_control_chars(trimmed) {
        return Err(PayoutError::InvalidRequest);
    }
    Ok(trimmed.to_string())
}

fn validate_receiver(address: &str) -> Result<String, PayoutError> {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        return Err(PayoutError::InvalidRequest);
    }
    if contains_control_chars(trimmed) {
        return Err(PayoutError::InvalidRequest);
    }
    Ok(trimmed.to_string())
}

fn validate_optional_genesis_path(value: Option<&str>) -> Result<Option<String>, PayoutError> {
    match value {
        None => Ok(None),
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(PayoutError::InvalidRequest);
            }
            if contains_control_chars(trimmed) {
                return Err(PayoutError::InvalidRequest);
            }
            Ok(Some(trimmed.to_string()))
        }
    }
}

/// Builds the future CLI argv for a `send` invocation. Does not spawn or execute anything.
///
/// Returns separate argv elements suitable for `Command::new(argv[0]).args(&argv[1..])` — never a
/// shell string.
///
/// Verified core shape (MVP 3d-2b):
/// `{cli_path} send --data-dir {node_data_dir} [--genesis {genesis_path}] {receiver} {amount} {fee}`
///
/// When `genesis_path` is omitted, core defaults to `{node_data_dir}/genesis.toml`.
/// Fee is always included explicitly even though core defaults fee to 1.
#[allow(dead_code)] // called from CLI adapter when execution is enabled (MVP 3d-3+)
pub fn build_cli_send_args(
    request: &PayoutRequest,
    cli_config: &CliPayoutConfig,
) -> Result<Vec<String>, PayoutError> {
    if !request.dry_run {
        // Real payout argv construction is not enabled until MVP 3d-3+.
        return Err(PayoutError::InvalidRequest);
    }

    if request.amount == 0 || request.fee == 0 {
        return Err(PayoutError::InvalidRequest);
    }

    let cli_path = validate_config_path(cli_config.cli_path.as_deref())?;
    let node_data_dir = validate_config_path(cli_config.node_data_dir.as_deref())?;
    let genesis_path = validate_optional_genesis_path(cli_config.genesis_path.as_deref())?;
    let receiver = validate_receiver(&request.address)?;

    let mut argv = vec![
        cli_path,
        "send".to_string(),
        "--data-dir".to_string(),
        node_data_dir,
    ];

    if let Some(genesis_path) = genesis_path {
        argv.push("--genesis".to_string());
        argv.push(genesis_path);
    }

    argv.push(receiver);
    argv.push(request.amount.to_string());
    argv.push(request.fee.to_string());

    Ok(argv)
}

#[async_trait]
impl PayoutAdapter for CliPayoutAdapter {
    async fn submit_payout(&self, _request: PayoutRequest) -> Result<PayoutResult, PayoutError> {
        // MVP 3d-1/3d-2: skeleton only; does not call `build_cli_send_args` or execute CLI.
        Err(PayoutError::Disabled)
    }
}

/// Dry-run adapter: never sends TRIL, never reads secrets, never calls CLI/RPC.
pub struct DryRunPayoutAdapter;

impl DryRunPayoutAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DryRunPayoutAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PayoutAdapter for DryRunPayoutAdapter {
    async fn submit_payout(&self, _request: PayoutRequest) -> Result<PayoutResult, PayoutError> {
        Ok(PayoutResult {
            tx_hash: None,
            status: claim_status::DRY_RUN_ACCEPTED.to_string(),
            dry_run: true,
        })
    }
}

/// Production wiring: only the dry-run adapter is constructed (MVP 3d-2).
pub fn default_payout_adapter() -> Arc<dyn PayoutAdapter> {
    Arc::new(DryRunPayoutAdapter::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> PayoutRequest {
        PayoutRequest {
            address: "tl1test_receiver".to_string(),
            amount: 10,
            fee: 1,
            dry_run: true,
        }
    }

    fn sample_cli_config() -> CliPayoutConfig {
        CliPayoutConfig {
            cli_path: Some("/usr/local/bin/trilogicon".to_string()),
            node_data_dir: Some("/var/lib/trilogicon-testnet".to_string()),
            genesis_path: None,
            fixed_fee: 1,
        }
    }

    #[test]
    fn build_cli_send_args_builds_expected_argv_without_genesis() {
        let argv = build_cli_send_args(&sample_request(), &sample_cli_config()).expect("argv");
        assert_eq!(
            argv,
            vec![
                "/usr/local/bin/trilogicon".to_string(),
                "send".to_string(),
                "--data-dir".to_string(),
                "/var/lib/trilogicon-testnet".to_string(),
                "tl1test_receiver".to_string(),
                "10".to_string(),
                "1".to_string(),
            ]
        );
    }

    #[test]
    fn build_cli_send_args_builds_expected_argv_with_genesis() {
        let config = CliPayoutConfig {
            genesis_path: Some("/var/lib/trilogicon-testnet/genesis.toml".to_string()),
            ..sample_cli_config()
        };
        let argv = build_cli_send_args(&sample_request(), &config).expect("argv");
        assert_eq!(
            argv,
            vec![
                "/usr/local/bin/trilogicon".to_string(),
                "send".to_string(),
                "--data-dir".to_string(),
                "/var/lib/trilogicon-testnet".to_string(),
                "--genesis".to_string(),
                "/var/lib/trilogicon-testnet/genesis.toml".to_string(),
                "tl1test_receiver".to_string(),
                "10".to_string(),
                "1".to_string(),
            ]
        );
    }

    #[test]
    fn build_cli_send_args_missing_genesis_is_allowed() {
        let config = CliPayoutConfig {
            genesis_path: None,
            ..sample_cli_config()
        };
        let argv = build_cli_send_args(&sample_request(), &config).expect("argv");
        assert!(!argv.iter().any(|arg| arg == "--genesis"));
    }

    #[test]
    fn build_cli_send_args_includes_fee_explicitly() {
        let request = PayoutRequest {
            fee: 1,
            ..sample_request()
        };
        let argv = build_cli_send_args(&request, &sample_cli_config()).expect("argv");
        assert_eq!(argv.last(), Some(&"1".to_string()));
    }

    #[test]
    fn build_cli_send_args_returns_separate_argv_elements() {
        let argv = build_cli_send_args(&sample_request(), &sample_cli_config()).expect("argv");
        assert_eq!(argv.len(), 7);
        assert!(!argv.iter().any(|arg| arg.contains(' ')));
        assert_ne!(argv[1], "send --data-dir");

        let config = CliPayoutConfig {
            genesis_path: Some("/var/lib/trilogicon-testnet/genesis.toml".to_string()),
            ..sample_cli_config()
        };
        let argv = build_cli_send_args(&sample_request(), &config).expect("argv");
        assert_eq!(argv.len(), 9);
        assert!(!argv.iter().any(|arg| arg.contains(' ')));
    }

    #[test]
    fn build_cli_send_args_rejects_empty_genesis_path_when_provided() {
        let config = CliPayoutConfig {
            genesis_path: Some("   ".to_string()),
            ..sample_cli_config()
        };
        let err = build_cli_send_args(&sample_request(), &config).expect_err("empty genesis");
        assert_eq!(err, PayoutError::InvalidRequest);
    }

    #[test]
    fn build_cli_send_args_rejects_control_char_in_genesis_path() {
        let config = CliPayoutConfig {
            genesis_path: Some("/var/genesis\u{0}.toml".to_string()),
            ..sample_cli_config()
        };
        let err = build_cli_send_args(&sample_request(), &config).expect_err("genesis control");
        assert_eq!(err, PayoutError::InvalidRequest);
    }

    #[test]
    fn build_cli_send_args_rejects_missing_cli_path() {
        let config = CliPayoutConfig {
            cli_path: None,
            ..sample_cli_config()
        };
        let err = build_cli_send_args(&sample_request(), &config).expect_err("missing cli");
        assert_eq!(err, PayoutError::Misconfigured);
    }

    #[test]
    fn build_cli_send_args_rejects_empty_cli_path() {
        let config = CliPayoutConfig {
            cli_path: Some("   ".to_string()),
            ..sample_cli_config()
        };
        let err = build_cli_send_args(&sample_request(), &config).expect_err("empty cli");
        assert_eq!(err, PayoutError::Misconfigured);
    }

    #[test]
    fn build_cli_send_args_rejects_missing_node_data_dir() {
        let config = CliPayoutConfig {
            node_data_dir: None,
            ..sample_cli_config()
        };
        let err = build_cli_send_args(&sample_request(), &config).expect_err("missing data dir");
        assert_eq!(err, PayoutError::Misconfigured);
    }

    #[test]
    fn build_cli_send_args_rejects_empty_node_data_dir() {
        let config = CliPayoutConfig {
            node_data_dir: Some("\t".to_string()),
            ..sample_cli_config()
        };
        let err = build_cli_send_args(&sample_request(), &config).expect_err("empty data dir");
        assert_eq!(err, PayoutError::Misconfigured);
    }

    #[test]
    fn build_cli_send_args_rejects_empty_receiver() {
        let request = PayoutRequest {
            address: "   ".to_string(),
            ..sample_request()
        };
        let err = build_cli_send_args(&request, &sample_cli_config()).expect_err("empty receiver");
        assert_eq!(err, PayoutError::InvalidRequest);
    }

    #[test]
    fn build_cli_send_args_rejects_zero_amount() {
        let request = PayoutRequest {
            amount: 0,
            ..sample_request()
        };
        let err = build_cli_send_args(&request, &sample_cli_config()).expect_err("zero amount");
        assert_eq!(err, PayoutError::InvalidRequest);
    }

    #[test]
    fn build_cli_send_args_rejects_zero_fee() {
        let request = PayoutRequest {
            fee: 0,
            ..sample_request()
        };
        let err = build_cli_send_args(&request, &sample_cli_config()).expect_err("zero fee");
        assert_eq!(err, PayoutError::InvalidRequest);
    }

    #[test]
    fn build_cli_send_args_rejects_non_dry_run_request() {
        let request = PayoutRequest {
            dry_run: false,
            ..sample_request()
        };
        let err = build_cli_send_args(&request, &sample_cli_config()).expect_err("live request");
        assert_eq!(err, PayoutError::InvalidRequest);
    }

    #[test]
    fn build_cli_send_args_rejects_control_char_in_cli_path() {
        let config = CliPayoutConfig {
            cli_path: Some("/bin/tri\u{0}l".to_string()),
            ..sample_cli_config()
        };
        let err = build_cli_send_args(&sample_request(), &config).expect_err("cli control");
        assert_eq!(err, PayoutError::InvalidRequest);
    }

    #[test]
    fn build_cli_send_args_rejects_control_char_in_node_data_dir() {
        let config = CliPayoutConfig {
            node_data_dir: Some("/var/data\u{0}".to_string()),
            ..sample_cli_config()
        };
        let err = build_cli_send_args(&sample_request(), &config).expect_err("data dir control");
        assert_eq!(err, PayoutError::InvalidRequest);
    }

    #[test]
    fn build_cli_send_args_rejects_control_char_in_receiver() {
        let request = PayoutRequest {
            address: "addr\u{1}b".to_string(),
            ..sample_request()
        };
        let err =
            build_cli_send_args(&request, &sample_cli_config()).expect_err("receiver control");
        assert_eq!(err, PayoutError::InvalidRequest);
    }

    #[tokio::test]
    async fn cli_adapter_submit_payout_is_disabled_without_execution() {
        let adapter = CliPayoutAdapter::new(CliPayoutConfig {
            cli_path: None,
            node_data_dir: None,
            genesis_path: None,
            fixed_fee: 1,
        });
        let err = adapter
            .submit_payout(PayoutRequest {
                address: "tl1test_receiver".to_string(),
                amount: 10,
                fee: 1,
                dry_run: false,
            })
            .await
            .expect_err("disabled");
        assert_eq!(err, PayoutError::Disabled);
    }

    #[tokio::test]
    async fn dry_run_adapter_returns_accepted_without_tx_hash() {
        let adapter = DryRunPayoutAdapter::new();
        let result = adapter
            .submit_payout(PayoutRequest {
                address: "tl1test_example".to_string(),
                amount: 10,
                fee: 1,
                dry_run: true,
            })
            .await
            .expect("dry-run payout");

        assert!(result.dry_run);
        assert!(result.tx_hash.is_none());
        assert_eq!(result.status, "dry_run_accepted");
    }

    #[tokio::test]
    async fn default_payout_adapter_uses_dry_run() {
        let adapter = default_payout_adapter();
        let result = adapter
            .submit_payout(PayoutRequest {
                address: "tl1test_example".to_string(),
                amount: 10,
                fee: 1,
                dry_run: true,
            })
            .await
            .expect("dry-run default adapter");

        assert!(result.dry_run);
        assert_eq!(result.status, "dry_run_accepted");
    }
}
