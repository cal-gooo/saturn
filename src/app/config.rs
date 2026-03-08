use std::{env, str::FromStr};

use secp256k1::{PublicKey, Secp256k1, SecretKey, XOnlyPublicKey};

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
        let merchant_signing_secret_key = read_string(
            "APP__MERCHANT_SIGNING_SECRET_KEY",
            "1111111111111111111111111111111111111111111111111111111111111111",
        );
        let derived_merchant_nostr_pubkey =
            derive_nostr_pubkey(&merchant_signing_secret_key)?;
        let merchant_nostr_pubkey = match env::var("APP__MERCHANT_NOSTR_PUBKEY") {
            Ok(value) => normalize_nostr_pubkey(&value)?,
            Err(_) => derived_merchant_nostr_pubkey.clone(),
        };
        if merchant_nostr_pubkey != derived_merchant_nostr_pubkey {
            return Err(ApiError::internal(
                "APP__MERCHANT_NOSTR_PUBKEY must match the pubkey derived from APP__MERCHANT_SIGNING_SECRET_KEY",
            ));
        }

        Ok(Self {
            server_addr: read_string("APP__SERVER_ADDR", "0.0.0.0:3000"),
            database_url: read_string(
                "APP__DATABASE_URL",
                "postgres://postgres:postgres@localhost:5432/a2a_commerce",
            ),
            merchant_name: read_string("APP__MERCHANT_NAME", "Example Merchant"),
            merchant_nostr_pubkey,
            merchant_signing_secret_key,
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
        let merchant_signing_secret_key =
            "1111111111111111111111111111111111111111111111111111111111111111".to_owned();
        Self {
            server_addr: "127.0.0.1:3000".into(),
            database_url: "postgres://postgres:postgres@localhost:5432/a2a_commerce".into(),
            merchant_name: "Test Merchant".into(),
            merchant_nostr_pubkey: derive_nostr_pubkey(&merchant_signing_secret_key)
                .expect("test merchant nostr pubkey should derive from secret key"),
            merchant_signing_secret_key,
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

fn normalize_nostr_pubkey(value: &str) -> AppResult<String> {
    let bytes = hex::decode(value.trim()).map_err(|error| {
        ApiError::internal(format!("invalid APP__MERCHANT_NOSTR_PUBKEY hex: {error}"))
    })?;

    match bytes.len() {
        32 => {
            let x_only = XOnlyPublicKey::from_byte_array(bytes.try_into().map_err(|_| {
                ApiError::internal("APP__MERCHANT_NOSTR_PUBKEY must be 32 bytes")
            })?)
            .map_err(|error| {
                ApiError::internal(format!("invalid APP__MERCHANT_NOSTR_PUBKEY: {error}"))
            })?;
            Ok(hex::encode(x_only.serialize()))
        }
        33 => {
            let public_key = PublicKey::from_slice(&bytes).map_err(|error| {
                ApiError::internal(format!("invalid APP__MERCHANT_NOSTR_PUBKEY: {error}"))
            })?;
            let (x_only, _) = public_key.x_only_public_key();
            Ok(hex::encode(x_only.serialize()))
        }
        _ => Err(ApiError::internal(
            "APP__MERCHANT_NOSTR_PUBKEY must be a 32-byte x-only or 33-byte compressed secp256k1 public key",
        )),
    }
}

fn derive_nostr_pubkey(secret_key_hex: &str) -> AppResult<String> {
    let secret_key_bytes = hex::decode(secret_key_hex).map_err(|error| {
        ApiError::internal(format!(
            "invalid APP__MERCHANT_SIGNING_SECRET_KEY hex: {error}"
        ))
    })?;
    let secret_key = SecretKey::from_byte_array(
        secret_key_bytes.try_into().map_err(|_| {
            ApiError::internal("APP__MERCHANT_SIGNING_SECRET_KEY must be 32 bytes")
        })?,
    )
    .map_err(|error| {
        ApiError::internal(format!(
            "invalid APP__MERCHANT_SIGNING_SECRET_KEY: {error}"
        ))
    })?;
    let secp = Secp256k1::signing_only();
    let public_key = PublicKey::from_secret_key(&secp, &secret_key);
    let (x_only, _) = public_key.x_only_public_key();
    Ok(hex::encode(x_only.serialize()))
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::AppConfig;
    use crate::security::signing::derive_public_key;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn normalizes_compressed_pubkey_input() {
        let _guard = env_lock().lock().expect("env lock should work");
        unsafe {
            std::env::set_var(
                "APP__MERCHANT_SIGNING_SECRET_KEY",
                "1111111111111111111111111111111111111111111111111111111111111111",
            );
            std::env::set_var(
                "APP__MERCHANT_NOSTR_PUBKEY",
                derive_public_key(
                    "1111111111111111111111111111111111111111111111111111111111111111",
                )
                .expect("compressed test pubkey should derive"),
            );
        }

        let config = AppConfig::from_env().expect("config should load");
        assert_eq!(
            config.merchant_nostr_pubkey,
            "4f355bdcb7cc0af728ef3cceb9615d90684bb5b2ca5f859ab0f0b704075871aa"
        );

        unsafe {
            std::env::remove_var("APP__MERCHANT_SIGNING_SECRET_KEY");
            std::env::remove_var("APP__MERCHANT_NOSTR_PUBKEY");
        }
    }

    #[test]
    fn rejects_pubkey_secret_mismatch() {
        let _guard = env_lock().lock().expect("env lock should work");
        unsafe {
            std::env::set_var(
                "APP__MERCHANT_SIGNING_SECRET_KEY",
                "1111111111111111111111111111111111111111111111111111111111111111",
            );
            std::env::set_var(
                "APP__MERCHANT_NOSTR_PUBKEY",
                "f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9",
            );
        }

        let error = AppConfig::from_env().expect_err("config should reject mismatch");
        assert!(
            error
                .message
                .contains("APP__MERCHANT_NOSTR_PUBKEY must match"),
            "unexpected error: {}",
            error.message
        );

        unsafe {
            std::env::remove_var("APP__MERCHANT_SIGNING_SECRET_KEY");
            std::env::remove_var("APP__MERCHANT_NOSTR_PUBKEY");
        }
    }
}
