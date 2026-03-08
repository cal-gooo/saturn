use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::errors::{ApiError, AppResult};

pub fn canonical_json(value: &Value) -> AppResult<String> {
    let canonical = canonicalize_value(value);
    serde_json::to_string(&canonical)
        .map_err(|error| ApiError::internal(format!("failed to serialize canonical json: {error}")))
}

pub fn payload_hash_without_signature(value: &Value) -> AppResult<[u8; 32]> {
    let mut sanitized = value.clone();
    let object = sanitized
        .as_object_mut()
        .ok_or_else(|| ApiError::bad_request("signed request body must be a JSON object"))?;
    object.remove("signature");
    let canonical = canonical_json(&sanitized)?;
    let digest = Sha256::digest(canonical.as_bytes());
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    Ok(bytes)
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().cloned().collect();
            keys.sort();
            let mut sorted = Map::new();
            for key in keys {
                if let Some(value) = object.get(&key) {
                    sorted.insert(key, canonicalize_value(value));
                }
            }
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_value).collect()),
        _ => value.clone(),
    }
}
