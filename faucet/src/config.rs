//! Environment-backed configuration with safe defaults.

use std::env;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8787";
const DEFAULT_DATABASE_URL: &str = "sqlite:faucet.db";
const DEFAULT_DRY_RUN: bool = true;
const DEFAULT_CLAIM_AMOUNT: i64 = 10;
const DEFAULT_COOLDOWN_SECONDS: i64 = 3600;
const DEFAULT_IP_COOLDOWN_SECONDS: i64 = 3600;
const DEFAULT_ENABLE_PAYOUTS: bool = false;
const DEFAULT_NETWORK: &str = "testnet";
const DEFAULT_NODE_MODE: &str = "disabled";
const DEFAULT_FIXED_FEE: i64 = 1;
const DEFAULT_MAX_DAILY_CLAIMS: i64 = 1000;
const DEFAULT_MAX_DAILY_AMOUNT: i64 = 10000;

const ENABLE_PAYOUTS_RESERVED_MSG: &str =
    "FAUCET_ENABLE_PAYOUTS is reserved for a later payout milestone; MVP 3a is config-only";

/// Supported faucet network identifiers (MVP 3a: testnet only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network(String);

impl Network {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(raw: &str) -> Result<Self, String> {
        let network = raw.trim();
        if network.eq_ignore_ascii_case(DEFAULT_NETWORK) {
            Ok(Self(DEFAULT_NETWORK.to_string()))
        } else {
            Err(format!(
                "FAUCET_NETWORK must be \"{DEFAULT_NETWORK}\" (got {network:?})"
            ))
        }
    }
}

/// How the faucet would reach a Trilogicon node (parsed only in MVP 3a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeMode {
    Disabled,
    Cli,
    Rpc,
}

impl NodeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Cli => "cli",
            Self::Rpc => "rpc",
        }
    }

    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "disabled" => Ok(Self::Disabled),
            "cli" => Ok(Self::Cli),
            "rpc" => Ok(Self::Rpc),
            _ => Err(
                "FAUCET_NODE_MODE must be one of: disabled, cli, rpc (case insensitive)"
                    .to_string(),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub dry_run: bool,
    pub claim_amount: i64,
    pub cooldown_seconds: i64,
    pub ip_cooldown_seconds: i64,
    pub enable_payouts: bool,
    pub network: Network,
    pub wallet_seed_path: Option<String>,
    pub node_mode: NodeMode,
    pub node_cli_path: Option<String>,
    pub node_data_dir: Option<String>,
    pub fixed_fee: i64,
    pub max_daily_claims: i64,
    pub max_daily_amount: i64,
}

/// Heuristic for MVP 3b path validation; not applied at startup in MVP 3a.
#[allow(dead_code)] // wired at startup in MVP 3b
pub fn seed_path_looks_inside_repo(path: &str) -> bool {
    let p = path.trim();
    if p.is_empty() {
        return false;
    }
    // Relative paths are treated as potentially inside the repo working tree.
    !(p.starts_with('/') || p.starts_with('\\'))
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn env_i64(key: &str, default: i64) -> Result<i64, String> {
    match env::var(key) {
        Ok(v) => v
            .trim()
            .parse()
            .map_err(|_| format!("{key} must be a valid integer")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{key} must be valid UTF-8")),
    }
}

fn env_bool(key: &str, default: bool) -> Result<bool, String> {
    match env::var(key) {
        Ok(v) => parse_bool(&v).ok_or_else(|| {
            format!("{key} must be one of: 1, 0, true, false, yes, no, on, off (case insensitive)")
        }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{key} must be valid UTF-8")),
    }
}

fn env_optional_string(key: &str) -> Result<Option<String>, String> {
    match env::var(key) {
        Ok(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{key} must be valid UTF-8")),
    }
}

fn env_string(key: &str, default: &str) -> Result<String, String> {
    match env::var(key) {
        Ok(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                Ok(default.to_string())
            } else {
                Ok(trimmed.to_string())
            }
        }
        Err(env::VarError::NotPresent) => Ok(default.to_string()),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{key} must be valid UTF-8")),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let bind_addr =
            env::var("FAUCET_BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());

        let database_url =
            env::var("FAUCET_DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());

        let dry_run = match env::var("FAUCET_DRY_RUN") {
            Ok(v) => parse_bool(&v).ok_or_else(|| {
                "FAUCET_DRY_RUN must be one of: 1, 0, true, false, yes, no, on, off (case insensitive)"
                    .to_string()
            })?,
            Err(env::VarError::NotPresent) => DEFAULT_DRY_RUN,
            Err(env::VarError::NotUnicode(_)) => {
                return Err("FAUCET_DRY_RUN must be valid UTF-8".to_string());
            }
        };

        let claim_amount = env_i64("FAUCET_CLAIM_AMOUNT", DEFAULT_CLAIM_AMOUNT)?;
        if claim_amount <= 0 {
            return Err("FAUCET_CLAIM_AMOUNT must be positive".to_string());
        }

        let cooldown_seconds = env_i64("FAUCET_COOLDOWN_SECONDS", DEFAULT_COOLDOWN_SECONDS)?;
        if cooldown_seconds < 0 {
            return Err("FAUCET_COOLDOWN_SECONDS must be non-negative".to_string());
        }

        let ip_cooldown_seconds =
            env_i64("FAUCET_IP_COOLDOWN_SECONDS", DEFAULT_IP_COOLDOWN_SECONDS)?;
        if ip_cooldown_seconds < 0 {
            return Err("FAUCET_IP_COOLDOWN_SECONDS must be non-negative".to_string());
        }

        let enable_payouts = env_bool("FAUCET_ENABLE_PAYOUTS", DEFAULT_ENABLE_PAYOUTS)?;
        if enable_payouts {
            return Err(ENABLE_PAYOUTS_RESERVED_MSG.to_string());
        }

        let network = Network::parse(&env_string("FAUCET_NETWORK", DEFAULT_NETWORK)?)?;

        let wallet_seed_path = env_optional_string("FAUCET_WALLET_SEED_PATH")?;
        let node_mode = NodeMode::parse(&env_string("FAUCET_NODE_MODE", DEFAULT_NODE_MODE)?)?;
        let node_cli_path = env_optional_string("FAUCET_NODE_CLI_PATH")?;
        let node_data_dir = env_optional_string("FAUCET_NODE_DATA_DIR")?;

        let fixed_fee = env_i64("FAUCET_FIXED_FEE", DEFAULT_FIXED_FEE)?;
        if fixed_fee <= 0 {
            return Err("FAUCET_FIXED_FEE must be positive".to_string());
        }

        let max_daily_claims = env_i64("FAUCET_MAX_DAILY_CLAIMS", DEFAULT_MAX_DAILY_CLAIMS)?;
        if max_daily_claims <= 0 {
            return Err("FAUCET_MAX_DAILY_CLAIMS must be positive".to_string());
        }

        let max_daily_amount = env_i64("FAUCET_MAX_DAILY_AMOUNT", DEFAULT_MAX_DAILY_AMOUNT)?;
        if max_daily_amount <= 0 {
            return Err("FAUCET_MAX_DAILY_AMOUNT must be positive".to_string());
        }

        Ok(Self {
            bind_addr,
            database_url,
            dry_run,
            claim_amount,
            cooldown_seconds,
            ip_cooldown_seconds,
            enable_payouts,
            network,
            wallet_seed_path,
            node_mode,
            node_cli_path,
            node_data_dir,
            fixed_fee,
            max_daily_claims,
            max_daily_amount,
        })
    }

    /// Safe public view: payouts are never enabled in MVP 3a.
    pub fn payouts_enabled_status(&self) -> bool {
        false
    }

    #[cfg(test)]
    pub fn test_defaults(dry_run: bool, cooldown_seconds: i64, ip_cooldown_seconds: i64) -> Self {
        Self {
            bind_addr: "127.0.0.1:0".to_string(),
            database_url: "sqlite::memory:".to_string(),
            dry_run,
            claim_amount: DEFAULT_CLAIM_AMOUNT,
            cooldown_seconds,
            ip_cooldown_seconds,
            enable_payouts: DEFAULT_ENABLE_PAYOUTS,
            network: Network::parse(DEFAULT_NETWORK).expect("testnet"),
            wallet_seed_path: None,
            node_mode: NodeMode::Disabled,
            node_cli_path: None,
            node_data_dir: None,
            fixed_fee: DEFAULT_FIXED_FEE,
            max_daily_claims: DEFAULT_MAX_DAILY_CLAIMS,
            max_daily_amount: DEFAULT_MAX_DAILY_AMOUNT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env test lock poisoned")
    }

    fn clear_faucet_env() {
        for key in [
            "FAUCET_BIND_ADDR",
            "FAUCET_DATABASE_URL",
            "FAUCET_DRY_RUN",
            "FAUCET_CLAIM_AMOUNT",
            "FAUCET_COOLDOWN_SECONDS",
            "FAUCET_IP_COOLDOWN_SECONDS",
            "FAUCET_ENABLE_PAYOUTS",
            "FAUCET_NETWORK",
            "FAUCET_WALLET_SEED_PATH",
            "FAUCET_NODE_MODE",
            "FAUCET_NODE_CLI_PATH",
            "FAUCET_NODE_DATA_DIR",
            "FAUCET_FIXED_FEE",
            "FAUCET_MAX_DAILY_CLAIMS",
            "FAUCET_MAX_DAILY_AMOUNT",
        ] {
            env::remove_var(key);
        }
    }

    #[test]
    fn defaults_when_unset() {
        let _g = env_lock();
        clear_faucet_env();

        let c = Config::from_env().expect("defaults should parse");
        assert_eq!(c.bind_addr, DEFAULT_BIND_ADDR);
        assert_eq!(c.database_url, DEFAULT_DATABASE_URL);
        assert!(c.dry_run);
        assert_eq!(c.claim_amount, DEFAULT_CLAIM_AMOUNT);
        assert_eq!(c.cooldown_seconds, DEFAULT_COOLDOWN_SECONDS);
        assert_eq!(c.ip_cooldown_seconds, DEFAULT_IP_COOLDOWN_SECONDS);
        assert!(!c.enable_payouts);
        assert_eq!(c.network.as_str(), "testnet");
        assert_eq!(c.node_mode, NodeMode::Disabled);
        assert_eq!(c.fixed_fee, DEFAULT_FIXED_FEE);
        assert_eq!(c.max_daily_claims, DEFAULT_MAX_DAILY_CLAIMS);
        assert_eq!(c.max_daily_amount, DEFAULT_MAX_DAILY_AMOUNT);
        assert!(c.wallet_seed_path.is_none());
    }

    #[test]
    fn invalid_ip_cooldown_fails_startup() {
        let _g = env_lock();
        clear_faucet_env();
        env::set_var("FAUCET_IP_COOLDOWN_SECONDS", "not-a-number");
        let err = Config::from_env().expect_err("invalid ip cooldown");
        assert!(err.contains("FAUCET_IP_COOLDOWN_SECONDS"));
    }

    #[test]
    fn dry_run_false_parses() {
        let _g = env_lock();
        clear_faucet_env();
        env::set_var("FAUCET_DRY_RUN", "false");
        let c = Config::from_env().expect("parse");
        assert!(!c.dry_run);
    }

    #[test]
    fn mainnet_network_fails_startup() {
        let _g = env_lock();
        clear_faucet_env();
        env::set_var("FAUCET_NETWORK", "mainnet");
        let err = Config::from_env().expect_err("mainnet");
        assert!(err.contains("FAUCET_NETWORK"));
        assert!(err.contains("testnet"));
    }

    #[test]
    fn enable_payouts_true_fails_startup() {
        let _g = env_lock();
        clear_faucet_env();
        env::set_var("FAUCET_ENABLE_PAYOUTS", "true");
        let err = Config::from_env().expect_err("enable payouts");
        assert!(err.contains("FAUCET_ENABLE_PAYOUTS"));
        assert!(err.contains("MVP 3a"));
    }

    #[test]
    fn fixed_fee_zero_fails() {
        let _g = env_lock();
        clear_faucet_env();
        env::set_var("FAUCET_FIXED_FEE", "0");
        let err = Config::from_env().expect_err("fixed fee");
        assert!(err.contains("FAUCET_FIXED_FEE"));
    }

    #[test]
    fn max_daily_claims_zero_fails() {
        let _g = env_lock();
        clear_faucet_env();
        env::set_var("FAUCET_MAX_DAILY_CLAIMS", "0");
        let err = Config::from_env().expect_err("max daily claims");
        assert!(err.contains("FAUCET_MAX_DAILY_CLAIMS"));
    }

    #[test]
    fn max_daily_amount_zero_fails() {
        let _g = env_lock();
        clear_faucet_env();
        env::set_var("FAUCET_MAX_DAILY_AMOUNT", "0");
        let err = Config::from_env().expect_err("max daily amount");
        assert!(err.contains("FAUCET_MAX_DAILY_AMOUNT"));
    }

    #[test]
    fn invalid_node_mode_fails() {
        let _g = env_lock();
        clear_faucet_env();
        env::set_var("FAUCET_NODE_MODE", "websocket");
        let err = Config::from_env().expect_err("node mode");
        assert!(err.contains("FAUCET_NODE_MODE"));
    }

    #[test]
    fn wallet_seed_path_parses_without_reading() {
        let _g = env_lock();
        clear_faucet_env();
        env::set_var(
            "FAUCET_WALLET_SEED_PATH",
            "/etc/trilogicon/faucet/wallet.seed",
        );
        let c = Config::from_env().expect("parse seed path");
        assert_eq!(
            c.wallet_seed_path.as_deref(),
            Some("/etc/trilogicon/faucet/wallet.seed")
        );
    }

    #[test]
    fn seed_path_looks_inside_repo_relative() {
        assert!(seed_path_looks_inside_repo("wallet.seed"));
        assert!(seed_path_looks_inside_repo("./secrets/wallet.seed"));
    }

    #[test]
    fn seed_path_looks_inside_repo_absolute() {
        assert!(!seed_path_looks_inside_repo("/etc/trilogicon/wallet.seed"));
    }
}
