//! Payout adapter boundary for future CLI/RPC integration (MVP 3c: dry-run only).

use async_trait::async_trait;
use std::sync::Arc;

/// Input for a single faucet payout attempt.
#[allow(dead_code)] // fields consumed by future CLI/RPC adapters (MVP 3d+)
pub struct PayoutRequest {
    pub address: String,
    pub amount: u64,
    pub fee: u64,
    pub dry_run: bool,
}

pub struct PayoutResult {
    pub tx_hash: Option<String>,
    pub status: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // variants returned by future CLI/RPC adapters (MVP 3d+)
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

/// Production wiring: only the dry-run adapter is constructed in MVP 3c.
pub fn default_payout_adapter() -> Arc<dyn PayoutAdapter> {
    Arc::new(DryRunPayoutAdapter::new())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
