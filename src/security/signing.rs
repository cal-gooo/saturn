use chrono::{DateTime, Utc};
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey, ecdsa::Signature};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    errors::{ApiError, AppResult},
    security::canonical::payload_hash_without_signature,
};

#[derive(Debug, Clone)]
pub struct EnvelopeMetadata {
    pub message_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub nonce: String,
    pub public_key: String,
    pub signature: String,
}

impl EnvelopeMetadata {
    pub fn from_value(value: &Value) -> AppResult<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| ApiError::bad_request("signed request body must be an object"))?;

        let message_id = object
            .get("message_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("message_id is required"))?
            .parse()
            .map_err(|error| ApiError::bad_request(format!("invalid message_id: {error}")))?;

        let timestamp = object
            .get("timestamp")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("timestamp is required"))?;
        let timestamp = DateTime::parse_from_rfc3339(timestamp)
            .map_err(|error| ApiError::bad_request(format!("invalid timestamp: {error}")))?
            .with_timezone(&Utc);

        let nonce = object
            .get("nonce")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("nonce is required"))?
            .to_owned();

        let public_key = object
            .get("public_key")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("public_key is required"))?
            .to_owned();

        let signature = object
            .get("signature")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("signature is required"))?
            .to_owned();

        Ok(Self {
            message_id,
            timestamp,
            nonce,
            public_key,
            signature,
        })
    }
}

pub fn verify_signature(value: &Value) -> AppResult<EnvelopeMetadata> {
    let metadata = EnvelopeMetadata::from_value(value)?;
    let signature_bytes = hex::decode(&metadata.signature)
        .map_err(|error| ApiError::signature_invalid(format!("signature hex invalid: {error}")))?;
    let signature = Signature::from_compact(&signature_bytes)
        .map_err(|error| ApiError::signature_invalid(format!("signature invalid: {error}")))?;
    let public_key_bytes = hex::decode(&metadata.public_key)
        .map_err(|error| ApiError::signature_invalid(format!("public key hex invalid: {error}")))?;
    let public_key = PublicKey::from_slice(&public_key_bytes).map_err(|error| {
        ApiError::signature_invalid(format!("public key encoding invalid: {error}"))
    })?;
    let digest = payload_hash_without_signature(value)?;
    let message = Message::from_digest(digest);
    let secp = Secp256k1::verification_only();
    secp.verify_ecdsa(message, &signature, &public_key)
        .map_err(|error| {
            ApiError::signature_invalid(format!("signature verification failed: {error}"))
        })?;
    Ok(metadata)
}

pub fn derive_public_key(secret_key_hex: &str) -> AppResult<String> {
    let secret_key_bytes = hex::decode(secret_key_hex)
        .map_err(|error| ApiError::internal(format!("invalid secret key hex: {error}")))?;
    let secret_key = SecretKey::from_byte_array(
        secret_key_bytes
            .try_into()
            .map_err(|_| ApiError::internal("secret key must be 32 bytes"))?,
    )
    .map_err(|error| ApiError::internal(format!("invalid secret key: {error}")))?;
    let secp = Secp256k1::signing_only();
    Ok(hex::encode(
        PublicKey::from_secret_key(&secp, &secret_key).serialize(),
    ))
}

pub fn sign_value(value: &mut Value, secret_key_hex: &str) -> AppResult<()> {
    let secret_key_bytes = hex::decode(secret_key_hex)
        .map_err(|error| ApiError::internal(format!("invalid secret key hex: {error}")))?;
    let secret_key = SecretKey::from_byte_array(
        secret_key_bytes
            .try_into()
            .map_err(|_| ApiError::internal("secret key must be 32 bytes"))?,
    )
    .map_err(|error| ApiError::internal(format!("invalid secret key: {error}")))?;
    let secp = Secp256k1::signing_only();
    let public_key = PublicKey::from_secret_key(&secp, &secret_key);
    {
        let object = value
            .as_object_mut()
            .ok_or_else(|| ApiError::bad_request("signed payload must be an object"))?;
        object.insert(
            "public_key".to_owned(),
            Value::String(hex::encode(public_key.serialize())),
        );
    }
    let digest = payload_hash_without_signature(value)?;
    let message = Message::from_digest(digest);
    let signature = secp.sign_ecdsa(message, &secret_key);
    value
        .as_object_mut()
        .ok_or_else(|| ApiError::bad_request("signed payload must be an object"))?
        .insert(
            "signature".to_owned(),
            Value::String(hex::encode(signature.serialize_compact())),
        );
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    use super::{sign_value, verify_signature};
    use crate::errors::AppResult;

    #[test]
    fn signs_and_verifies_payloads() -> AppResult<()> {
        let mut payload = json!({
            "message_id": Uuid::new_v4(),
            "timestamp": Utc::now(),
            "nonce": "nonce-1",
            "signature": "",
            "buyer_nostr_pubkey": "buyer",
            "seller_nostr_pubkey": "seller",
        });

        sign_value(
            &mut payload,
            "1111111111111111111111111111111111111111111111111111111111111111",
        )?;
        verify_signature(&payload)?;
        Ok(())
    }

    #[test]
    fn rejects_tampered_payloads() -> AppResult<()> {
        let mut payload = json!({
            "message_id": Uuid::new_v4(),
            "timestamp": Utc::now(),
            "nonce": "nonce-2",
            "signature": "",
            "amount_sats": 100,
        });

        sign_value(
            &mut payload,
            "1111111111111111111111111111111111111111111111111111111111111111",
        )?;

        payload["amount_sats"] = json!(200);
        assert!(verify_signature(&payload).is_err());
        Ok(())
    }
}
