use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use validator::Validate;

use crate::errors::{ApiError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Validate)]
pub struct LineItem {
    #[validate(length(min = 1, max = 64))]
    pub sku: String,
    #[validate(length(min = 1, max = 255))]
    pub description: String,
    #[validate(range(min = 1))]
    pub quantity: i64,
    #[validate(range(min = 1))]
    pub unit_price_sats: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettlementPreference {
    LightningOnly,
    LightningWithOnchainFallback,
}

impl SettlementPreference {
    pub fn accepted_rails(self) -> Vec<PaymentRail> {
        match self {
            Self::LightningOnly => vec![PaymentRail::Lightning],
            Self::LightningWithOnchainFallback => {
                vec![PaymentRail::Lightning, PaymentRail::OnChain]
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PaymentRail {
    Lightning,
    OnChain,
}

impl fmt::Display for PaymentRail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lightning => write!(f, "lightning"),
            Self::OnChain => write!(f, "onchain"),
        }
    }
}

impl FromStr for PaymentRail {
    type Err = ApiError;

    fn from_str(value: &str) -> AppResult<Self> {
        match value {
            "lightning" => Ok(Self::Lightning),
            "onchain" => Ok(Self::OnChain),
            _ => Err(ApiError::internal(format!("unknown payment rail: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrderState {
    Created,
    Quoted,
    PaymentPending,
    Paid,
    Fulfilled,
    Expired,
    Cancelled,
    Disputed,
}

impl fmt::Display for OrderState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Quoted => write!(f, "quoted"),
            Self::PaymentPending => write!(f, "payment_pending"),
            Self::Paid => write!(f, "paid"),
            Self::Fulfilled => write!(f, "fulfilled"),
            Self::Expired => write!(f, "expired"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::Disputed => write!(f, "disputed"),
        }
    }
}

impl FromStr for OrderState {
    type Err = ApiError;

    fn from_str(value: &str) -> AppResult<Self> {
        match value {
            "created" => Ok(Self::Created),
            "quoted" => Ok(Self::Quoted),
            "payment_pending" => Ok(Self::PaymentPending),
            "paid" => Ok(Self::Paid),
            "fulfilled" => Ok(Self::Fulfilled),
            "expired" => Ok(Self::Expired),
            "cancelled" => Ok(Self::Cancelled),
            "disputed" => Ok(Self::Disputed),
            _ => Err(ApiError::internal(format!("unknown order state: {value}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PaymentFinality {
    Settled,
    Confirmed,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettlementProof {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub id: Uuid,
    pub order_id: Uuid,
    pub buyer_pubkey: String,
    pub seller_pubkey: String,
    pub items: Vec<LineItem>,
    pub settlement_preference: SettlementPreference,
    pub callback_relays: Vec<String>,
    pub buyer_reference: Option<String>,
    pub total_sats: i64,
    pub status: OrderState,
    pub expires_at: DateTime<Utc>,
    pub quote_lock_until: DateTime<Utc>,
    pub accepted_rails: Vec<PaymentRail>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub quote_id: Uuid,
    pub buyer_pubkey: String,
    pub seller_pubkey: String,
    pub state: OrderState,
    pub selected_rail: Option<PaymentRail>,
    pub checkout_idempotency_key: Option<String>,
    pub payment_confirm_idempotency_key: Option<String>,
    pub lightning_invoice: Option<String>,
    pub lightning_payment_hash: Option<String>,
    pub onchain_address: Option<String>,
    pub payment_amount_sats: Option<i64>,
    pub settlement_proof: Option<SettlementProof>,
    pub onchain_confirmations: Option<u32>,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub id: Uuid,
    pub order_id: Uuid,
    pub rail: PaymentRail,
    pub receipt_hash: String,
    pub nostr_event_id: Option<String>,
    pub finality: PaymentFinality,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

pub fn total_sats(items: &[LineItem]) -> i64 {
    items
        .iter()
        .map(|item| item.quantity.saturating_mul(item.unit_price_sats))
        .sum()
}
