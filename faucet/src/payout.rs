//! Payout adapter boundary for future CLI/RPC integration (MVP 3c+: dry-run live; CLI skeleton in 3d-1).

use async_trait::async_trait;
use std::sync::Arc;

/// Input for a single faucet payout attempt.
pub struct PayoutRequest {
    pub address: String,
    pub amount: u64,
    pub fee: u64,
    #[allow(dead_code)] // checked by future live adapters (MVP 3d-3+)
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
    BackendUnavailable,
    Rejected,
    Internal,
}

#[async_trait]
pub trait PayoutAdapter: Send + Sync {
    async fn submit_payout(&self, request: PayoutRequest) -> Result<PayoutResult, PayoutError>;
}

/// Safe parsed fields for a future CLI payout backend (no execution in MVP 3d-1).
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

/// Builds the future CLI argv for a `send` invocation. Does not spawn or execute anything.
///
/// Shape (to verify against `trilogicon-core` before MVP 3d-3):
/// `{cli_path} send --data-dir {node_data_dir} {receiver} {amount} {fee}`
#[allow(dead_code)] // called from CLI adapter when execution is enabled (MVP 3d-3+)
pub fn build_cli_send_args(
    request: &PayoutRequest,
    cli_config: &CliPayoutConfig,
) -> Result<Vec<String>, PayoutError> {
    let cli_path = cli_config
        .cli_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(PayoutError::Misconfigured)?;

    let node_data_dir = cli_config
        .node_data_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(PayoutError::Misconfigured)?;

    Ok(vec![
        cli_path.to_string(),
        "send".to_string(),
        "--data-dir".to_string(),
        node_data_dir.to_string(),
        request.address.clone(),
        request.amount.to_string(),
        request.fee.to_string(),
    ])
}

#[async_trait]
impl PayoutAdapter for CliPayoutAdapter {
    async fn submit_payout(&self, _request: PayoutRequest) -> Result<PayoutResult, PayoutError> {
        // MVP 3d-1: skeleton only; real CLI execution remains gated for MVP 3d-3+.
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

/// Production wiring: only the dry-run adapter is constructed (MVP 3d-1).
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
            dry_run: false,
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
    fn build_cli_send_args_rejects_missing_cli_path() {
        let config = CliPayoutConfig {
            cli_path: None,
            ..sample_cli_config()
        };
        let err = build_cli_send_args(&sample_request(), &config).expect_err("missing cli");
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

    #[tokio::test]
    async fn cli_adapter_submit_payout_is_disabled() {
        let adapter = CliPayoutAdapter::new(sample_cli_config());
        let err = adapter
            .submit_payout(sample_request())
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
