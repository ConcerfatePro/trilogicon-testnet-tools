//! Payout adapter boundary for future CLI/RPC integration (MVP 3c+: dry-run live; CLI skeleton in 3d-1).

use async_trait::async_trait;
use std::sync::Arc;

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

/// Builds the future CLI argv for a `send` invocation. Does not spawn or execute anything.
///
/// Returns separate argv elements suitable for `Command::new(argv[0]).args(&argv[1..])` — never a
/// shell string.
///
/// Shape (must be verified against `trilogicon-core` before MVP 3d-3):
/// `{cli_path} send --data-dir {node_data_dir} {receiver} {amount} {fee}`
///
/// TODO(MVP 3d-3): If the core CLI requires `--genesis`, add an explicit config field after
/// verifying the exact flag against `trilogicon-core`. Do not invent genesis behavior here.
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
    let receiver = validate_receiver(&request.address)?;

    Ok(vec![
        cli_path,
        "send".to_string(),
        "--data-dir".to_string(),
        node_data_dir,
        receiver,
        request.amount.to_string(),
        request.fee.to_string(),
    ])
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
            status: "dry_run_accepted".to_string(),
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
            fixed_fee: 1,
        }
    }

    #[test]
    fn build_cli_send_args_builds_expected_argv() {
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
    fn build_cli_send_args_returns_separate_argv_elements() {
        let argv = build_cli_send_args(&sample_request(), &sample_cli_config()).expect("argv");
        assert_eq!(argv.len(), 7);
        assert!(!argv.iter().any(|arg| arg.contains(' ')));
        assert_ne!(argv[1], "send --data-dir");
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
