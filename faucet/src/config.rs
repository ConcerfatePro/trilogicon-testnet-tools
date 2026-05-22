//! Environment-backed configuration with safe defaults.

use std::env;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8787";
const DEFAULT_DATABASE_URL: &str = "sqlite:faucet.db";
const DEFAULT_DRY_RUN: bool = true;
const DEFAULT_CLAIM_AMOUNT: i64 = 10;
const DEFAULT_COOLDOWN_SECONDS: i64 = 3600;
const DEFAULT_IP_COOLDOWN_SECONDS: i64 = 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub dry_run: bool,
    pub claim_amount: i64,
    pub cooldown_seconds: i64,
    pub ip_cooldown_seconds: i64,
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

        Ok(Self {
            bind_addr,
            database_url,
            dry_run,
            claim_amount,
            cooldown_seconds,
            ip_cooldown_seconds,
        })
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

    #[test]
    fn defaults_when_unset() {
        let _g = env_lock();
        // Clear relevant vars so from_env uses defaults (tests may inherit shell env).
        for key in [
            "FAUCET_BIND_ADDR",
            "FAUCET_DATABASE_URL",
            "FAUCET_DRY_RUN",
            "FAUCET_CLAIM_AMOUNT",
            "FAUCET_COOLDOWN_SECONDS",
            "FAUCET_IP_COOLDOWN_SECONDS",
        ] {
            env::remove_var(key);
        }

        let c = Config::from_env().expect("defaults should parse");
        assert_eq!(c.bind_addr, DEFAULT_BIND_ADDR);
        assert_eq!(c.database_url, DEFAULT_DATABASE_URL);
        assert!(c.dry_run);
        assert_eq!(c.claim_amount, DEFAULT_CLAIM_AMOUNT);
        assert_eq!(c.cooldown_seconds, DEFAULT_COOLDOWN_SECONDS);
        assert_eq!(c.ip_cooldown_seconds, DEFAULT_IP_COOLDOWN_SECONDS);
    }

    #[test]
    fn invalid_ip_cooldown_fails_startup() {
        let _g = env_lock();
        env::set_var("FAUCET_IP_COOLDOWN_SECONDS", "not-a-number");
        let err = Config::from_env().expect_err("invalid ip cooldown");
        assert!(err.contains("FAUCET_IP_COOLDOWN_SECONDS"));
        env::remove_var("FAUCET_IP_COOLDOWN_SECONDS");
    }

    #[test]
    fn dry_run_false_parses() {
        let _g = env_lock();
        env::remove_var("FAUCET_BIND_ADDR");
        env::remove_var("FAUCET_DATABASE_URL");
        env::remove_var("FAUCET_CLAIM_AMOUNT");
        env::remove_var("FAUCET_COOLDOWN_SECONDS");
        env::remove_var("FAUCET_IP_COOLDOWN_SECONDS");
        env::set_var("FAUCET_DRY_RUN", "false");
        let c = Config::from_env().expect("parse");
        assert!(!c.dry_run);
        env::remove_var("FAUCET_DRY_RUN");
    }
}
