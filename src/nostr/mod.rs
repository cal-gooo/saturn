use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::info;
use uuid::Uuid;

use crate::{
    api::schemas::NostrReference,
    app::config::AppConfig,
    domain::entities::{Order, PaymentRail, Quote, Receipt},
    errors::AppResult,
};

pub const CAPABILITY_ANNOUNCEMENT_KIND: u32 = 31_390;
pub const QUOTE_REFERENCE_KIND: u32 = 17_390;
pub const PAYMENT_RECEIPT_KIND: u32 = 17_391;
pub const STATUS_UPDATE_KIND: u32 = 17_392;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrEvent {
    pub kind: u32,
    pub pubkey: String,
    pub created_at: i64,
    pub tags: Vec<Vec<String>>,
    pub content: Value,
}

#[async_trait]
pub trait NostrPublisher: Send + Sync {
    async fn publish_capability(&self, config: &AppConfig) -> AppResult<NostrReference>;
    async fn publish_quote_reference(&self, quote: &Quote) -> AppResult<NostrReference>;
    async fn publish_receipt(
        &self,
        order: &Order,
        receipt: &Receipt,
        merchant_pubkey: &str,
    ) -> AppResult<NostrReference>;
}

#[derive(Debug, Clone)]
pub struct MockNostrPublisher {
    relays: Vec<String>,
}

impl MockNostrPublisher {
    pub fn new(relays: Vec<String>) -> Self {
        Self { relays }
    }

    fn event_id(event: &NostrEvent) -> String {
        let digest = Sha256::digest(event.content.to_string().as_bytes());
        hex::encode(digest)
    }
}

#[async_trait]
impl NostrPublisher for MockNostrPublisher {
    async fn publish_capability(&self, config: &AppConfig) -> AppResult<NostrReference> {
        let event = NostrEvent {
            kind: CAPABILITY_ANNOUNCEMENT_KIND,
            pubkey: config.merchant_nostr_pubkey.clone(),
            created_at: Utc::now().timestamp(),
            tags: vec![
                vec!["d".into(), "merchant-capabilities".into()],
                vec!["t".into(), "a2ac/v0.1".into()],
            ],
            content: json!({
                "protocol": "a2ac/0.1",
                "merchant_name": config.merchant_name,
                "relays": self.relays.clone(),
                "supported_rails": ["lightning", "onchain"],
            }),
        };
        let event_id = Self::event_id(&event);
        info!(event_id, "published capability event to configured relays");
        Ok(NostrReference {
            kind: CAPABILITY_ANNOUNCEMENT_KIND,
            event_id,
            relays: self.relays.clone(),
        })
    }

    async fn publish_quote_reference(&self, quote: &Quote) -> AppResult<NostrReference> {
        let event = NostrEvent {
            kind: QUOTE_REFERENCE_KIND,
            pubkey: quote.seller_pubkey.clone(),
            created_at: Utc::now().timestamp(),
            tags: vec![
                vec!["t".into(), "a2ac/v0.1".into()],
                vec!["q".into(), quote.id.to_string()],
                vec!["o".into(), quote.order_id.to_string()],
                vec!["p".into(), quote.buyer_pubkey.clone()],
                vec!["p".into(), quote.seller_pubkey.clone()],
            ],
            content: json!({
                "status": "quoted",
                "expires_at": quote.expires_at,
                "quote_lock_until": quote.quote_lock_until,
            }),
        };
        let event_id = Self::event_id(&event);
        info!(event_id, quote_id = %quote.id, "published quote reference");
        Ok(NostrReference {
            kind: QUOTE_REFERENCE_KIND,
            event_id,
            relays: self.relays.clone(),
        })
    }

    async fn publish_receipt(
        &self,
        order: &Order,
        receipt: &Receipt,
        merchant_pubkey: &str,
    ) -> AppResult<NostrReference> {
        let event = NostrEvent {
            kind: PAYMENT_RECEIPT_KIND,
            pubkey: merchant_pubkey.to_owned(),
            created_at: Utc::now().timestamp(),
            tags: vec![
                vec!["t".into(), "a2ac/v0.1".into()],
                vec!["o".into(), order.id.to_string()],
                vec!["q".into(), order.quote_id.to_string()],
                vec!["r".into(), receipt.rail.to_string()],
                vec!["x".into(), receipt.receipt_hash.clone()],
            ],
            content: receipt.payload.clone(),
        };
        let event_id = Self::event_id(&event);
        info!(event_id, order_id = %order.id, receipt_id = %receipt.id, "published receipt");
        Ok(NostrReference {
            kind: PAYMENT_RECEIPT_KIND,
            event_id,
            relays: self.relays.clone(),
        })
    }
}

pub type DynNostrPublisher = Arc<dyn NostrPublisher>;

pub fn experimental_event_kinds() -> Vec<u32> {
    vec![
        CAPABILITY_ANNOUNCEMENT_KIND,
        QUOTE_REFERENCE_KIND,
        PAYMENT_RECEIPT_KIND,
        STATUS_UPDATE_KIND,
    ]
}

pub fn quote_tags(quote_id: Uuid, order_id: Uuid, rail: Option<PaymentRail>) -> Vec<Vec<String>> {
    let mut tags = vec![
        vec!["q".into(), quote_id.to_string()],
        vec!["o".into(), order_id.to_string()],
    ];
    if let Some(rail) = rail {
        tags.push(vec!["r".into(), rail.to_string()]);
    }
    tags
}
