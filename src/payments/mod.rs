use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domain::entities::{PaymentFinality, SettlementProof},
    errors::{ApiError, AppResult},
};

#[derive(Debug, Clone)]
pub struct LightningInvoice {
    pub bolt11: String,
    pub payment_hash: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PaymentVerification {
    pub finality: PaymentFinality,
    pub settled_at: DateTime<Utc>,
    pub normalized_proof: SettlementProof,
}

#[async_trait]
pub trait LightningAdapter: Send + Sync {
    async fn create_invoice(
        &self,
        order_id: Uuid,
        amount_sats: i64,
        memo: &str,
    ) -> AppResult<LightningInvoice>;

    async fn verify_payment(
        &self,
        proof: &SettlementProof,
        expected_hash: Option<&str>,
        expected_amount_sats: i64,
    ) -> AppResult<PaymentVerification>;
}

#[async_trait]
pub trait OnChainAdapter: Send + Sync {
    async fn new_address(&self, order_id: Uuid) -> AppResult<String>;

    async fn verify_settlement(
        &self,
        proof: &SettlementProof,
        expected_amount_sats: i64,
        minimum_confirmations: u32,
    ) -> AppResult<PaymentVerification>;
}

#[derive(Debug, Clone, Default)]
pub struct MockLightningAdapter;

#[async_trait]
impl LightningAdapter for MockLightningAdapter {
    async fn create_invoice(
        &self,
        order_id: Uuid,
        amount_sats: i64,
        _memo: &str,
    ) -> AppResult<LightningInvoice> {
        let seed = Sha256::digest(order_id.as_bytes());
        let payment_hash = hex::encode(seed);
        Ok(LightningInvoice {
            bolt11: format!("lnbc{amount_sats}n1{}", &payment_hash[..24]),
            payment_hash,
            expires_at: Utc::now() + chrono::Duration::minutes(15),
        })
    }

    async fn verify_payment(
        &self,
        proof: &SettlementProof,
        expected_hash: Option<&str>,
        expected_amount_sats: i64,
    ) -> AppResult<PaymentVerification> {
        match proof {
            SettlementProof::Lightning {
                payment_hash,
                settled_at,
                amount_sats,
                preimage,
            } => {
                if *amount_sats != expected_amount_sats {
                    return Err(ApiError::payment_verification_failed(
                        "lightning amount does not match quote",
                    ));
                }
                if let Some(expected_hash) = expected_hash
                    && payment_hash != expected_hash
                {
                    return Err(ApiError::payment_verification_failed(
                        "lightning payment hash mismatch",
                    ));
                }
                if preimage.as_deref() == Some("invalid") {
                    return Err(ApiError::payment_verification_failed(
                        "invalid lightning preimage",
                    ));
                }
                Ok(PaymentVerification {
                    finality: PaymentFinality::Settled,
                    settled_at: *settled_at,
                    normalized_proof: proof.clone(),
                })
            }
            _ => Err(ApiError::payment_verification_failed(
                "lightning adapter received non-lightning proof",
            )),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MockOnChainAdapter;

#[async_trait]
impl OnChainAdapter for MockOnChainAdapter {
    async fn new_address(&self, order_id: Uuid) -> AppResult<String> {
        let address = format!("bcrt1q{}", &hex::encode(order_id.as_bytes())[..32]);
        Ok(address)
    }

    async fn verify_settlement(
        &self,
        proof: &SettlementProof,
        expected_amount_sats: i64,
        minimum_confirmations: u32,
    ) -> AppResult<PaymentVerification> {
        match proof {
            SettlementProof::OnChain {
                txid,
                amount_sats,
                confirmations,
                ..
            } => {
                if txid.len() < 8 {
                    return Err(ApiError::payment_verification_failed(
                        "transaction id is too short",
                    ));
                }
                if *amount_sats != expected_amount_sats {
                    return Err(ApiError::payment_verification_failed(
                        "on-chain amount does not match quote",
                    ));
                }

                let finality = if *confirmations >= minimum_confirmations {
                    PaymentFinality::Confirmed
                } else {
                    PaymentFinality::Pending
                };
                Ok(PaymentVerification {
                    finality,
                    settled_at: Utc::now(),
                    normalized_proof: proof.clone(),
                })
            }
            _ => Err(ApiError::payment_verification_failed(
                "on-chain adapter received non-on-chain proof",
            )),
        }
    }
}

pub type DynLightningAdapter = Arc<dyn LightningAdapter>;
pub type DynOnChainAdapter = Arc<dyn OnChainAdapter>;

pub fn receipt_hash(payload: &serde_json::Value) -> String {
    let digest = Sha256::digest(payload.to_string().as_bytes());
    hex::encode(digest)
}

pub fn build_receipt_payload(
    order_id: Uuid,
    rail: &str,
    amount_sats: i64,
    finality: &PaymentFinality,
    settled_at: DateTime<Utc>,
) -> serde_json::Value {
    json!({
        "order_id": order_id,
        "rail": rail,
        "amount_sats": amount_sats,
        "finality": finality,
        "settled_at": settled_at,
    })
}
