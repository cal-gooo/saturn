use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ldk_node::{
    Builder as LdkNodeBuilder, Node as LdkNode,
    bitcoin::{Network as BitcoinNetwork, hashes::Hash as _},
    config::WALLET_KEYS_SEED_LEN,
    lightning::ln::channelmanager::PaymentId as LdkPaymentId,
    lightning_invoice::{Bolt11InvoiceDescription, Description},
    payment::{
        PaymentDirection as LdkPaymentDirection, PaymentKind as LdkPaymentKind,
        PaymentStatus as LdkPaymentStatus,
    },
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::task;
use tracing::info;
use uuid::Uuid;

use crate::{
    app::config::AppConfig,
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

pub struct LdkNodeLightningAdapter {
    node: Arc<LdkNode>,
    invoice_expiry_seconds: u32,
}

impl LdkNodeLightningAdapter {
    pub fn new(config: &AppConfig) -> AppResult<Self> {
        let mut builder = LdkNodeBuilder::new();
        builder.set_network(parse_ldk_network(&config.lightning_ldk_network)?);
        builder.set_entropy_seed_bytes(parse_ldk_seed_hex(&config.lightning_ldk_seed_hex)?);
        builder.set_storage_dir_path(config.lightning_ldk_storage_dir.clone());
        builder.set_chain_source_esplora(config.lightning_ldk_esplora_url.clone(), None);
        if let Some(rgs_url) = &config.lightning_ldk_rgs_url {
            builder.set_gossip_source_rgs(rgs_url.clone());
        }
        builder.set_log_facade_logger();

        let node = Arc::new(builder.build().map_err(|error| {
            ApiError::internal(format!("failed to build ldk-node adapter: {error}"))
        })?);
        node.start().map_err(|error| {
            ApiError::internal(format!("failed to start ldk-node adapter: {error}"))
        })?;

        info!(
            backend = "ldk-node",
            network = %config.lightning_ldk_network,
            storage_dir = %config.lightning_ldk_storage_dir,
            "started lightning adapter"
        );

        Ok(Self {
            node,
            invoice_expiry_seconds: config.lightning_invoice_expiry_seconds,
        })
    }
}

impl Drop for LdkNodeLightningAdapter {
    fn drop(&mut self) {
        let _ = self.node.stop();
    }
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
impl LightningAdapter for LdkNodeLightningAdapter {
    async fn create_invoice(
        &self,
        _order_id: Uuid,
        amount_sats: i64,
        memo: &str,
    ) -> AppResult<LightningInvoice> {
        if amount_sats <= 0 {
            return Err(ApiError::bad_request(
                "lightning invoice amount must be greater than zero",
            ));
        }

        let amount_msat = u64::try_from(amount_sats)
            .map_err(|_| ApiError::bad_request("lightning amount is too large"))?
            .saturating_mul(1_000);
        let memo = memo.to_owned();
        let invoice_expiry_seconds = self.invoice_expiry_seconds;
        let node = Arc::clone(&self.node);

        task::spawn_blocking(move || -> AppResult<LightningInvoice> {
            let description = Bolt11InvoiceDescription::Direct(
                Description::new(memo).map_err(|error| {
                    ApiError::internal(format!("invalid lightning invoice description: {error}"))
                })?,
            );
            let invoice = node
                .bolt11_payment()
                .receive(amount_msat, &description, invoice_expiry_seconds)
                .map_err(|error| {
                    ApiError::internal(format!("ldk-node failed to create invoice: {error}"))
                })?;

            Ok(LightningInvoice {
                bolt11: invoice.to_string(),
                payment_hash: hex::encode(invoice.payment_hash().as_byte_array()),
                expires_at: Utc::now()
                    + chrono::Duration::seconds(i64::from(invoice_expiry_seconds)),
            })
        })
        .await
        .map_err(|error| ApiError::internal(format!("lightning task failed: {error}")))?
    }

    async fn verify_payment(
        &self,
        proof: &SettlementProof,
        expected_hash: Option<&str>,
        expected_amount_sats: i64,
    ) -> AppResult<PaymentVerification> {
        let (payment_hash, provided_preimage, settled_at_hint, amount_sats) = match proof {
            SettlementProof::Lightning {
                payment_hash,
                preimage,
                settled_at,
                amount_sats,
            } => (payment_hash.clone(), preimage.clone(), *settled_at, *amount_sats),
            _ => {
                return Err(ApiError::payment_verification_failed(
                    "lightning adapter received non-lightning proof",
                ));
            }
        };

        if amount_sats != expected_amount_sats {
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

        let payment_hash_bytes = decode_payment_hash(&payment_hash)?;
        let node = Arc::clone(&self.node);

        task::spawn_blocking(move || -> AppResult<PaymentVerification> {
            let payment = node
                .payment(&LdkPaymentId(payment_hash_bytes))
                .ok_or_else(|| {
                    ApiError::payment_verification_failed(
                        "ldk-node has no record of the supplied lightning payment hash",
                    )
                })?;

            if payment.direction != LdkPaymentDirection::Inbound {
                return Err(ApiError::payment_verification_failed(
                    "lightning payment hash does not refer to an inbound invoice",
                ));
            }

            let (stored_hash, stored_preimage) = extract_ldk_bolt11_details(&payment.kind)
                .ok_or_else(|| {
                    ApiError::payment_verification_failed(
                        "ldk-node payment record is not a BOLT11 invoice",
                    )
                })?;

            if stored_hash != payment_hash {
                return Err(ApiError::payment_verification_failed(
                    "ldk-node payment hash does not match the supplied proof",
                ));
            }

            if let Some(actual_amount_msat) = payment.amount_msat {
                let actual_amount_sats = i64::try_from(actual_amount_msat / 1_000)
                    .map_err(|_| ApiError::payment_verification_failed("invalid payment amount"))?;
                if actual_amount_sats != expected_amount_sats {
                    return Err(ApiError::payment_verification_failed(
                        "ldk-node recorded amount does not match quote",
                    ));
                }
            }

            let settled_at = DateTime::<Utc>::from_timestamp(payment.latest_update_timestamp as i64, 0)
                .unwrap_or(settled_at_hint);

            match payment.status {
                LdkPaymentStatus::Pending => Err(ApiError::payment_verification_failed(
                    "lightning payment is still pending in ldk-node",
                )),
                LdkPaymentStatus::Failed => Err(ApiError::payment_verification_failed(
                    "lightning payment failed in ldk-node",
                )),
                LdkPaymentStatus::Succeeded => Ok(PaymentVerification {
                    finality: PaymentFinality::Settled,
                    settled_at,
                    normalized_proof: SettlementProof::Lightning {
                        payment_hash: stored_hash,
                        preimage: stored_preimage.or(provided_preimage),
                        settled_at,
                        amount_sats: expected_amount_sats,
                    },
                }),
            }
        })
        .await
        .map_err(|error| ApiError::internal(format!("lightning task failed: {error}")))?
    }
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

pub fn build_lightning_adapter(config: &AppConfig) -> AppResult<DynLightningAdapter> {
    match config.lightning_backend.as_str() {
        "mock" => Ok(Arc::new(MockLightningAdapter)),
        "ldk" | "ldk-node" => Ok(Arc::new(LdkNodeLightningAdapter::new(config)?)),
        other => Err(ApiError::internal(format!(
            "unknown APP__LIGHTNING_BACKEND value: {other}"
        ))),
    }
}

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

fn parse_ldk_network(value: &str) -> AppResult<BitcoinNetwork> {
    match value {
        "bitcoin" | "mainnet" => Ok(BitcoinNetwork::Bitcoin),
        "testnet" => Ok(BitcoinNetwork::Testnet),
        "signet" => Ok(BitcoinNetwork::Signet),
        "regtest" => Ok(BitcoinNetwork::Regtest),
        other => Err(ApiError::internal(format!(
            "unsupported APP__LIGHTNING_LDK_NETWORK value: {other}"
        ))),
    }
}

fn parse_ldk_seed_hex(seed_hex: &str) -> AppResult<[u8; WALLET_KEYS_SEED_LEN]> {
    let bytes = hex::decode(seed_hex).map_err(|error| {
        ApiError::internal(format!("invalid APP__LIGHTNING_LDK_SEED_HEX: {error}"))
    })?;
    bytes
        .try_into()
        .map_err(|_| ApiError::internal("APP__LIGHTNING_LDK_SEED_HEX must decode to 64 bytes"))
}

fn decode_payment_hash(payment_hash: &str) -> AppResult<[u8; 32]> {
    let bytes = hex::decode(payment_hash).map_err(|error| {
        ApiError::payment_verification_failed(format!("lightning payment hash hex invalid: {error}"))
    })?;
    bytes.try_into().map_err(|_| {
        ApiError::payment_verification_failed("lightning payment hash must be 32 bytes")
    })
}

fn extract_ldk_bolt11_details(kind: &LdkPaymentKind) -> Option<(String, Option<String>)> {
    match kind {
        LdkPaymentKind::Bolt11 { hash, preimage, .. }
        | LdkPaymentKind::Bolt11Jit { hash, preimage, .. } => Some((
            hex::encode(hash.0),
            preimage.map(|value| hex::encode(value.0)),
        )),
        _ => None,
    }
}
