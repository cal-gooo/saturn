use std::{env, str::FromStr};

use crate::errors::{ApiError, AppResult};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server_addr: String,
    pub database_url: String,
    pub merchant_name: String,
    pub merchant_nostr_pubkey: String,
    pub merchant_signing_secret_key: String,
    pub nostr_relays: Vec<String>,
    pub quote_ttl_seconds: u64,
    pub quote_lock_seconds: u64,
    pub max_clock_skew_seconds: u64,
    pub onchain_confirmations_required: u32,
    pub log_format: String,
}

impl AppConfig {
    pub fn from_env() -> AppResult<Self> {
        Ok(Self {
            server_addr: read_string("APP__SERVER_ADDR", "0.0.0.0:3000"),
            database_url: read_string(
                "APP__DATABASE_URL",
                "postgres://postgres:postgres@localhost:5432/a2a_commerce",
            ),
            merchant_name: read_string("APP__MERCHANT_NAME", "Example Merchant"),
            merchant_nostr_pubkey: read_string(
                "APP__MERCHANT_NOSTR_PUBKEY",
                "02aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            merchant_signing_secret_key: read_string(
                "APP__MERCHANT_SIGNING_SECRET_KEY",
                "1111111111111111111111111111111111111111111111111111111111111111",
            ),
            nostr_relays: read_string("APP__NOSTR_RELAYS", "wss://relay.damus.io,wss://nos.lol")
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect(),
            quote_ttl_seconds: read_parse("APP__QUOTE_TTL_SECONDS", 300)?,
            quote_lock_seconds: read_parse("APP__QUOTE_LOCK_SECONDS", 180)?,
            max_clock_skew_seconds: read_parse("APP__MAX_CLOCK_SKEW_SECONDS", 120)?,
            onchain_confirmations_required: read_parse(
                "APP__ONCHAIN_CONFIRMATIONS_REQUIRED",
                3_u32,
            )?,
            log_format: read_string("APP__LOG_FORMAT", "pretty"),
        })
    }

    pub fn for_tests() -> Self {
        Self {
            server_addr: "127.0.0.1:3000".into(),
            database_url: "postgres://postgres:postgres@localhost:5432/a2a_commerce".into(),
            merchant_name: "Test Merchant".into(),
            merchant_nostr_pubkey:
                "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".into(),
            merchant_signing_secret_key:
                "1111111111111111111111111111111111111111111111111111111111111111".into(),
            nostr_relays: vec!["wss://relay.damus.io".into(), "wss://nos.lol".into()],
            quote_ttl_seconds: 300,
            quote_lock_seconds: 180,
            max_clock_skew_seconds: 120,
            onchain_confirmations_required: 3,
            log_format: "pretty".into(),
        }
    }
}

fn read_string(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn read_parse<T>(key: &str, default: T) -> AppResult<T>
where
    T: Copy + FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(key) {
        Ok(value) => value
            .parse()
            .map_err(|error| ApiError::internal(format!("invalid {key}: {error}"))),
        Err(_) => Ok(default),
    }
}
