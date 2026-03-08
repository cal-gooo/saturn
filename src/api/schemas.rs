use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;

use crate::{
    domain::entities::{LineItem, OrderState, PaymentFinality, PaymentRail, SettlementPreference},
    errors::{ApiError, AppResult},
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SignedEnvelope<T> {
    pub message_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub nonce: String,
    pub public_key: String,
    pub signature: String,
    #[serde(flatten)]
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Validate)]
pub struct QuoteRequestPayload {
    #[validate(length(min = 32, max = 130))]
    pub buyer_nostr_pubkey: String,
    #[validate(length(min = 32, max = 130))]
    pub seller_nostr_pubkey: String,
    #[validate(length(min = 1))]
    pub callback_relays: Vec<String>,
    #[validate(length(min = 1))]
    pub items: Vec<LineItem>,
    pub settlement_preference: SettlementPreference,
    #[validate(length(min = 1, max = 128))]
    pub buyer_reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Validate)]
pub struct CheckoutIntentPayload {
    pub quote_id: Uuid,
    pub selected_rail: PaymentRail,
    #[validate(length(min = 1, max = 128))]
    pub buyer_reference: Option<String>,
    pub return_relays: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Validate)]
pub struct PaymentConfirmPayload {
    pub order_id: Uuid,
    pub rail: PaymentRail,
    pub settlement_proof: SettlementProofInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettlementProofInput {
    Lightning {
        payment_hash: String,
        preimage: Option<String>,
        settled_at: DateTime<Utc>,
        amount_sats: i64,
    },
    OnChain {
        txid: String,
        vout: u32,
        amount_sats: i64,
        confirmations: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NostrReference {
    pub kind: u32,
    pub event_id: String,
    pub relays: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QuoteResponse {
    pub quote_id: Uuid,
    pub order_id: Uuid,
    pub state: OrderState,
    pub total_sats: i64,
    pub expires_at: DateTime<Utc>,
    pub quote_lock_until: DateTime<Utc>,
    pub accepted_rails: Vec<PaymentRail>,
    pub nostr_quote_reference: NostrReference,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CheckoutIntentResponse {
    pub order_id: Uuid,
    pub quote_id: Uuid,
    pub state: OrderState,
    pub selected_rail: PaymentRail,
    pub lightning_invoice: String,
    pub lightning_payment_hash: String,
    pub onchain_fallback_address: Option<String>,
    pub quote_lock_until: DateTime<Utc>,
    pub required_onchain_confirmations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PaymentConfirmResponse {
    pub order_id: Uuid,
    pub receipt_id: Uuid,
    pub state: OrderState,
    pub finality: PaymentFinality,
    pub receipt_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrderResponse {
    pub order_id: Uuid,
    pub quote_id: Uuid,
    pub state: OrderState,
    pub selected_rail: Option<PaymentRail>,
    pub payment_amount_sats: Option<i64>,
    pub receipt_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CapabilitiesResponse {
    pub protocol: String,
    pub version: String,
    pub merchant_name: String,
    pub merchant_nostr_pubkey: String,
    pub relay_urls: Vec<String>,
    pub supported_rails: Vec<PaymentRail>,
    pub quote_ttl_seconds: u64,
    pub quote_lock_seconds: u64,
    pub onchain_confirmations_required: u32,
    pub experimental_event_kinds: Vec<u32>,
}

pub fn parse_json<T>(raw: Value) -> AppResult<T>
where
    T: DeserializeOwned + JsonSchema,
{
    let schema = schemars::schema_for!(T);
    let schema_value = serde_json::to_value(&schema)
        .map_err(|error| ApiError::internal(format!("failed to serialize schema: {error}")))?;
    let validator = jsonschema::validator_for(&schema_value)
        .map_err(|error| ApiError::internal(format!("failed to compile schema: {error}")))?;
    let validation_errors: Vec<String> =
        validator.iter_errors(&raw).map(|e| e.to_string()).collect();

    if !validation_errors.is_empty() {
        return Err(ApiError::schema_invalid(json!({
            "errors": validation_errors,
        })));
    }

    serde_json::from_value(raw)
        .map_err(|error| ApiError::bad_request(format!("failed to parse request body: {error}")))
}

pub fn validate_payload<T>(payload: &T) -> AppResult<()>
where
    T: Validate,
{
    payload.validate().map_err(|error| {
        ApiError::schema_invalid(json!({
            "errors": [error.to_string()],
        }))
    })
}
