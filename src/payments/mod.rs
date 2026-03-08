use std::{str::FromStr, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ldk_node::{
    Builder as LdkNodeBuilder, Node as LdkNode,
    bitcoin::{Network as BitcoinNetwork, Txid, hashes::Hash as _},
    config::WALLET_KEYS_SEED_LEN,
    lightning::ln::channelmanager::PaymentId as LdkPaymentId,
    lightning_invoice::{Bolt11InvoiceDescription, Description},
    payment::{
        PaymentDirection as LdkPaymentDirection, PaymentKind as LdkPaymentKind,
        PaymentStatus as LdkPaymentStatus,
    },
};
use reqwest::{Client as HttpClient, StatusCode as HttpStatusCode};
use serde::Deserialize;
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

struct LdkNodeHandle {
    node: LdkNode,
}

impl LdkNodeHandle {
    fn new(config: &AppConfig) -> AppResult<Self> {
        let mut builder = LdkNodeBuilder::new();
        builder.set_network(parse_ldk_network(&config.lightning_ldk_network)?);
        builder.set_entropy_seed_bytes(parse_ldk_seed_hex(&config.lightning_ldk_seed_hex)?);
        builder.set_storage_dir_path(config.lightning_ldk_storage_dir.clone());
        builder.set_chain_source_esplora(config.lightning_ldk_esplora_url.clone(), None);
        if let Some(rgs_url) = &config.lightning_ldk_rgs_url {
            builder.set_gossip_source_rgs(rgs_url.clone());
        }
        builder.set_log_facade_logger();

        let node = builder.build().map_err(|error| {
            ApiError::internal(format!("failed to build ldk-node adapter: {error}"))
        })?;
        node.start().map_err(|error| {
            ApiError::internal(format!("failed to start ldk-node adapter: {error}"))
        })?;

        info!(
            backend = "ldk-node",
            network = %config.lightning_ldk_network,
            storage_dir = %config.lightning_ldk_storage_dir,
            "started ldk-node runtime"
        );

        Ok(Self { node })
    }
}

impl Drop for LdkNodeHandle {
    fn drop(&mut self) {
        let _ = self.node.stop();
    }
}

pub struct LdkNodeLightningAdapter {
    handle: Arc<LdkNodeHandle>,
    invoice_expiry_seconds: u32,
}

impl LdkNodeLightningAdapter {
    fn new(handle: Arc<LdkNodeHandle>, invoice_expiry_seconds: u32) -> Self {
        Self {
            handle,
            invoice_expiry_seconds,
        }
    }
}

impl LdkNodeOnChainAdapter {
    fn new(handle: Arc<LdkNodeHandle>, esplora_base_url: String) -> Self {
        Self {
            handle,
            esplora_client: HttpClient::new(),
            esplora_base_url: normalize_esplora_base_url(&esplora_base_url),
        }
    }

    async fn fetch_transaction(&self, txid: &Txid) -> AppResult<EsploraTransaction> {
        let url = format!("{}/tx/{txid}", self.esplora_base_url);
        let response = self.esplora_client.get(&url).send().await.map_err(|error| {
            ApiError::internal(format!("failed to query esplora transaction: {error}"))
        })?;

        if response.status() == HttpStatusCode::NOT_FOUND {
            return Err(ApiError::payment_verification_failed(
                "on-chain transaction was not found in the configured Esplora backend",
            ));
        }

        response.error_for_status_ref().map_err(|error| {
            ApiError::internal(format!("esplora transaction query failed: {error}"))
        })?;
        response.json().await.map_err(|error| {
            ApiError::internal(format!("failed to decode esplora transaction response: {error}"))
        })
    }

    async fn fetch_tip_height(&self) -> AppResult<u32> {
        let url = format!("{}/blocks/tip/height", self.esplora_base_url);
        let response = self.esplora_client.get(&url).send().await.map_err(|error| {
            ApiError::internal(format!("failed to query esplora tip height: {error}"))
        })?;
        response.error_for_status_ref().map_err(|error| {
            ApiError::internal(format!("esplora tip height query failed: {error}"))
        })?;
        let height = response.text().await.map_err(|error| {
            ApiError::internal(format!("failed to read esplora tip height response: {error}"))
        })?;
        height.trim().parse().map_err(|error| {
            ApiError::internal(format!("invalid esplora tip height response: {error}"))
        })
    }
}

pub struct LdkNodeOnChainAdapter {
    handle: Arc<LdkNodeHandle>,
    esplora_client: HttpClient,
    esplora_base_url: String,
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
        let handle = Arc::clone(&self.handle);

        task::spawn_blocking(move || -> AppResult<LightningInvoice> {
            let description = Bolt11InvoiceDescription::Direct(
                Description::new(memo).map_err(|error| {
                    ApiError::internal(format!("invalid lightning invoice description: {error}"))
                })?,
            );
            let invoice = handle
                .node
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
        let handle = Arc::clone(&self.handle);

        task::spawn_blocking(move || -> AppResult<PaymentVerification> {
            let payment = handle
                .node
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
        expected_address: &str,
        expected_amount_sats: i64,
        minimum_confirmations: u32,
    ) -> AppResult<PaymentVerification>;
}

#[async_trait]
impl OnChainAdapter for LdkNodeOnChainAdapter {
    async fn new_address(&self, _order_id: Uuid) -> AppResult<String> {
        let handle = Arc::clone(&self.handle);
        task::spawn_blocking(move || {
            handle
                .node
                .onchain_payment()
                .new_address()
                .map(|address| address.to_string())
                .map_err(|error| {
                    ApiError::internal(format!("ldk-node failed to derive on-chain address: {error}"))
                })
        })
        .await
        .map_err(|error| ApiError::internal(format!("on-chain task failed: {error}")))?
    }

    async fn verify_settlement(
        &self,
        proof: &SettlementProof,
        expected_address: &str,
        expected_amount_sats: i64,
        minimum_confirmations: u32,
    ) -> AppResult<PaymentVerification> {
        let (txid, vout, amount_sats) = match proof {
            SettlementProof::OnChain {
                txid,
                vout,
                amount_sats,
                ..
            } => (parse_onchain_txid(txid)?, *vout, *amount_sats),
            _ => {
                return Err(ApiError::payment_verification_failed(
                    "on-chain adapter received non-on-chain proof",
                ));
            }
        };

        if amount_sats != expected_amount_sats {
            return Err(ApiError::payment_verification_failed(
                "on-chain amount does not match quote",
            ));
        }

        let transaction = self.fetch_transaction(&txid).await?;
        let tip_height = if transaction.status.confirmed {
            Some(self.fetch_tip_height().await?)
        } else {
            None
        };
        verify_onchain_transaction(
            &txid.to_string(),
            vout,
            expected_address,
            expected_amount_sats,
            minimum_confirmations,
            &transaction,
            tip_height,
            Utc::now(),
        )
    }
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
        _expected_address: &str,
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

pub fn build_payment_adapters(
    config: &AppConfig,
) -> AppResult<(DynLightningAdapter, DynOnChainAdapter)> {
    let shared_ldk = if uses_ldk_backend(&config.lightning_backend)
        || uses_ldk_backend(&config.onchain_backend)
    {
        Some(Arc::new(LdkNodeHandle::new(config)?))
    } else {
        None
    };

    let lightning_adapter: DynLightningAdapter = match config.lightning_backend.as_str() {
        "mock" => Arc::new(MockLightningAdapter),
        "ldk" | "ldk-node" => Arc::new(LdkNodeLightningAdapter::new(
            shared_ldk
                .clone()
                .ok_or_else(|| ApiError::internal("missing shared ldk-node runtime"))?,
            config.lightning_invoice_expiry_seconds,
        )),
        other => {
            return Err(ApiError::internal(format!(
                "unknown APP__LIGHTNING_BACKEND value: {other}"
            )));
        }
    };

    let onchain_adapter: DynOnChainAdapter = match config.onchain_backend.as_str() {
        "mock" => Arc::new(MockOnChainAdapter),
        "ldk" | "ldk-node" => Arc::new(LdkNodeOnChainAdapter::new(
            shared_ldk
                .clone()
                .ok_or_else(|| ApiError::internal("missing shared ldk-node runtime"))?,
            config.lightning_ldk_esplora_url.clone(),
        )),
        other => {
            return Err(ApiError::internal(format!(
                "unknown APP__ONCHAIN_BACKEND value: {other}"
            )));
        }
    };

    Ok((lightning_adapter, onchain_adapter))
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

fn verify_onchain_transaction(
    txid: &str,
    vout: u32,
    expected_address: &str,
    expected_amount_sats: i64,
    minimum_confirmations: u32,
    transaction: &EsploraTransaction,
    tip_height: Option<u32>,
    observed_at: DateTime<Utc>,
) -> AppResult<PaymentVerification> {
    let output = transaction.vout.get(vout as usize).ok_or_else(|| {
        ApiError::payment_verification_failed("on-chain transaction output index is out of range")
    })?;
    let output_address = output.scriptpubkey_address.as_deref().ok_or_else(|| {
        ApiError::payment_verification_failed(
            "on-chain transaction output is missing a standard address",
        )
    })?;
    if output_address != expected_address {
        return Err(ApiError::payment_verification_failed(
            "on-chain transaction output does not match the order address",
        ));
    }

    let output_value_sats = i64::try_from(output.value)
        .map_err(|_| ApiError::payment_verification_failed("invalid on-chain output amount"))?;
    if output_value_sats != expected_amount_sats {
        return Err(ApiError::payment_verification_failed(
            "on-chain transaction output amount does not match quote",
        ));
    }

    let confirmations =
        confirmation_count(&transaction.status, tip_height.unwrap_or_default())?;
    let settled_at = transaction
        .status
        .block_time
        .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        .unwrap_or(observed_at);
    let finality = if confirmations >= minimum_confirmations {
        PaymentFinality::Confirmed
    } else {
        PaymentFinality::Pending
    };

    Ok(PaymentVerification {
        finality,
        settled_at,
        normalized_proof: SettlementProof::OnChain {
            txid: txid.to_owned(),
            vout,
            amount_sats: expected_amount_sats,
            confirmations,
        },
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

fn parse_onchain_txid(txid: &str) -> AppResult<Txid> {
    Txid::from_str(txid).map_err(|error| {
        ApiError::payment_verification_failed(format!("invalid on-chain transaction id: {error}"))
    })
}

fn normalize_esplora_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_owned()
}

fn uses_ldk_backend(backend: &str) -> bool {
    matches!(backend, "ldk" | "ldk-node")
}

fn confirmation_count(status: &EsploraTransactionStatus, tip_height: u32) -> AppResult<u32> {
    match status {
        EsploraTransactionStatus {
            confirmed: true,
            block_height: Some(block_height),
            ..
        } => Ok(tip_height.saturating_sub(*block_height).saturating_add(1)),
        EsploraTransactionStatus {
            confirmed: true,
            block_height: None,
            ..
        } => Err(ApiError::internal(
            "esplora marked transaction confirmed without a block height",
        )),
        _ => Ok(0),
    }
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

#[derive(Debug, Deserialize)]
struct EsploraTransaction {
    vout: Vec<EsploraTransactionOutput>,
    status: EsploraTransactionStatus,
}

#[derive(Debug, Deserialize)]
struct EsploraTransactionOutput {
    value: u64,
    scriptpubkey_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EsploraTransactionStatus {
    confirmed: bool,
    block_height: Option<u32>,
    block_time: Option<i64>,
}

#[cfg(test)]
mod tests {
    use std::{env, net::TcpListener, str::FromStr, time::Duration};

    use axum::{
        Router,
        body::Body,
        http::{Method, Request},
    };
    use chrono::{TimeZone, Utc};
    use electrsd::corepc_node::{self, Node as BitcoinD};
    use electrsd::ElectrsD;
    use http_body_util::BodyExt;
    use ldk_node::{
        Builder as LdkNodeBuilder, Event,
        bitcoin::{Address, Amount, Network as BitcoinNetwork, Txid, address::NetworkUnchecked},
        lightning::ln::msgs::SocketAddress,
        lightning_invoice::Bolt11Invoice,
    };
    use serde_json::{Value, json};
    use tokio::time::sleep;
    use tower::util::ServiceExt;

    use super::*;
    use crate::{
        app::{AppState, build_router},
        nostr::MockNostrPublisher,
        persistence::{
            InMemoryNonceRepository, InMemoryOrderRepository, InMemoryQuoteRepository,
            InMemoryReceiptRepository,
        },
        privacy::DisabledCoinjoinClient,
        security::signing::{derive_public_key, sign_value},
    };

    fn sample_transaction(address: &str, value: u64, status: EsploraTransactionStatus) -> EsploraTransaction {
        EsploraTransaction {
            vout: vec![EsploraTransactionOutput {
                value,
                scriptpubkey_address: Some(address.to_owned()),
            }],
            status,
        }
    }

    #[test]
    fn build_payment_adapters_supports_mock_backends() {
        let config = AppConfig::for_tests();
        let adapters = build_payment_adapters(&config);
        assert!(adapters.is_ok(), "mock backends should build without live services");
    }

    #[test]
    fn build_payment_adapters_rejects_unknown_onchain_backend() {
        let mut config = AppConfig::for_tests();
        config.onchain_backend = "wat".into();

        let error = match build_payment_adapters(&config) {
            Ok(_) => panic!("unknown backend should fail"),
            Err(error) => error,
        };
        assert!(error.message.contains("APP__ONCHAIN_BACKEND"));
    }

    #[test]
    fn verify_onchain_transaction_requires_matching_address() {
        let transaction = sample_transaction(
            "bcrt1qexpected000000000000000000000000000000000",
            21_000,
            EsploraTransactionStatus {
                confirmed: true,
                block_height: Some(100),
                block_time: Some(1_700_000_000),
            },
        );

        let error = verify_onchain_transaction(
            "00".repeat(32).as_str(),
            0,
            "bcrt1qdifferent0000000000000000000000000000000",
            21_000,
            3,
            &transaction,
            Some(102),
            Utc::now(),
        )
        .expect_err("address mismatch should fail");

        assert!(error.message.contains("does not match the order address"));
    }

    #[test]
    fn verify_onchain_transaction_reports_confirmed_finality() {
        let transaction = sample_transaction(
            "bcrt1qexpected000000000000000000000000000000000",
            21_000,
            EsploraTransactionStatus {
                confirmed: true,
                block_height: Some(100),
                block_time: Some(1_700_000_000),
            },
        );

        let verification = verify_onchain_transaction(
            &"11".repeat(32),
            0,
            "bcrt1qexpected000000000000000000000000000000000",
            21_000,
            3,
            &transaction,
            Some(102),
            Utc.timestamp_opt(1_700_000_100, 0).single().expect("timestamp"),
        )
        .expect("confirmed transaction should verify");

        assert_eq!(verification.finality, PaymentFinality::Confirmed);
        assert_eq!(
            verification.normalized_proof,
            SettlementProof::OnChain {
                txid: "11".repeat(32),
                vout: 0,
                amount_sats: 21_000,
                confirmations: 3,
            }
        );
        assert_eq!(
            verification.settled_at,
            Utc.timestamp_opt(1_700_000_000, 0).single().expect("block time"),
        );
    }

    #[test]
    fn verify_onchain_transaction_reports_pending_for_unconfirmed_txs() {
        let observed_at = Utc.timestamp_opt(1_700_000_100, 0).single().expect("timestamp");
        let transaction = sample_transaction(
            "bcrt1qexpected000000000000000000000000000000000",
            21_000,
            EsploraTransactionStatus {
                confirmed: false,
                block_height: None,
                block_time: None,
            },
        );

        let verification = verify_onchain_transaction(
            &"22".repeat(32),
            0,
            "bcrt1qexpected000000000000000000000000000000000",
            21_000,
            1,
            &transaction,
            None,
            observed_at,
        )
        .expect("unconfirmed transaction should still normalize");

        assert_eq!(verification.finality, PaymentFinality::Pending);
        assert_eq!(verification.settled_at, observed_at);
        assert_eq!(
            verification.normalized_proof,
            SettlementProof::OnChain {
                txid: "22".repeat(32),
                vout: 0,
                amount_sats: 21_000,
                confirmations: 0,
            }
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires downloaded bitcoind/electrs binaries and a live regtest environment"]
    async fn ldk_lightning_adapter_round_trips_real_payment() {
        let (bitcoind, electrsd) = setup_bitcoind_and_electrsd();
        let rpc = &bitcoind.client;
        let _ = rpc.create_wallet("saturn_lightning_regtest");
        let _ = rpc.load_wallet("saturn_lightning_regtest");
        generate_blocks_and_wait(rpc, &electrsd, 101).await;

        let esplora_url = format!(
            "http://{}",
            electrsd
                .esplora_url
                .as_ref()
                .expect("electrsd should expose esplora")
        );

        let payer = build_test_ldk_node(&esplora_url, 41, None);
        let receiver_handle = Arc::new(build_test_ldk_handle(
            &esplora_url,
            42,
            Some(random_socket_address()),
        ));
        let receiver_adapter = LdkNodeLightningAdapter::new(Arc::clone(&receiver_handle), 900);

        let payer_funding_address = payer
            .onchain_payment()
            .new_address()
            .expect("payer funding address");
        let payer_funding_txid = rpc
            .send_to_address(&payer_funding_address, Amount::from_sat(1_000_000))
            .expect("bitcoind should fund the payer")
            .0
            .parse::<Txid>()
            .expect("funding txid should parse");
        wait_for_esplora_tx(&esplora_url, &payer_funding_txid).await;
        generate_blocks_and_wait(rpc, &electrsd, 1).await;
        payer.sync_wallets().expect("payer wallets should sync");

        payer
            .open_channel(
                receiver_handle.node.node_id(),
                receiver_handle
                    .node
                    .listening_addresses()
                    .expect("receiver should have listening addresses")
                    .first()
                    .expect("receiver should expose one address")
                    .clone(),
                500_000,
                None,
                None,
            )
            .expect("payer should open a private channel");

        wait_for_event(&payer, |event| matches!(event, Event::ChannelPending { .. })).await;
        wait_for_event(&receiver_handle.node, |event| {
            matches!(event, Event::ChannelPending { .. })
        })
        .await;
        generate_blocks_and_wait(rpc, &electrsd, 6).await;
        payer.sync_wallets().expect("payer wallets should resync");
        receiver_handle
            .node
            .sync_wallets()
            .expect("receiver wallets should sync");
        wait_for_event(&payer, |event| matches!(event, Event::ChannelReady { .. })).await;
        wait_for_event(&receiver_handle.node, |event| {
            matches!(event, Event::ChannelReady { .. })
        })
        .await;

        let invoice = receiver_adapter
            .create_invoice(Uuid::new_v4(), 21_000, "saturn regtest lightning")
            .await
            .expect("receiver should create a real invoice");
        let parsed_invoice = Bolt11Invoice::from_str(&invoice.bolt11).expect("invoice should parse");
        payer
            .bolt11_payment()
            .send(&parsed_invoice, None)
            .expect("payer should send invoice");
        wait_for_event(&payer, |event| matches!(event, Event::PaymentSuccessful { .. })).await;
        wait_for_event(&receiver_handle.node, |event| {
            matches!(event, Event::PaymentReceived { .. })
        })
        .await;

        let proof = SettlementProof::Lightning {
            payment_hash: invoice.payment_hash.clone(),
            preimage: None,
            settled_at: Utc::now(),
            amount_sats: 21_000,
        };
        let verification = wait_for_lightning_verification(
            &receiver_adapter,
            &proof,
            Some(invoice.payment_hash.as_str()),
            21_000,
        )
        .await;

        assert_eq!(verification.finality, PaymentFinality::Settled);
        match verification.normalized_proof {
            SettlementProof::Lightning {
                payment_hash,
                preimage,
                amount_sats,
                ..
            } => {
                assert_eq!(payment_hash, invoice.payment_hash);
                assert_eq!(amount_sats, 21_000);
                assert!(preimage.is_some(), "ldk-node should expose a settled preimage");
            }
            other => panic!("expected normalized lightning proof, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires downloaded bitcoind/electrs binaries and a live regtest environment"]
    async fn saturn_router_completes_real_lightning_checkout() {
        let (bitcoind, electrsd) = setup_bitcoind_and_electrsd();
        let rpc = &bitcoind.client;
        let _ = rpc.create_wallet("saturn_router_regtest");
        let _ = rpc.load_wallet("saturn_router_regtest");
        generate_blocks_and_wait(rpc, &electrsd, 101).await;

        let esplora_url = format!(
            "http://{}",
            electrsd
                .esplora_url
                .as_ref()
                .expect("electrsd should expose esplora")
        );

        let payer = build_test_ldk_node(&esplora_url, 61, None);
        let receiver_handle = Arc::new(build_test_ldk_handle(
            &esplora_url,
            62,
            Some(random_socket_address()),
        ));
        let payer_funding_address = payer
            .onchain_payment()
            .new_address()
            .expect("payer funding address");
        let payer_funding_txid = rpc
            .send_to_address(&payer_funding_address, Amount::from_sat(1_000_000))
            .expect("bitcoind should fund the payer")
            .0
            .parse::<Txid>()
            .expect("funding txid should parse");
        wait_for_esplora_tx(&esplora_url, &payer_funding_txid).await;
        generate_blocks_and_wait(rpc, &electrsd, 1).await;
        payer.sync_wallets().expect("payer wallets should sync");

        payer
            .open_channel(
                receiver_handle.node.node_id(),
                receiver_handle
                    .node
                    .listening_addresses()
                    .expect("receiver should have listening addresses")
                    .first()
                    .expect("receiver should expose a listening address")
                    .clone(),
                500_000,
                None,
                None,
            )
            .expect("payer should open a private channel");

        wait_for_event(&payer, |event| matches!(event, Event::ChannelPending { .. })).await;
        wait_for_event(&receiver_handle.node, |event| {
            matches!(event, Event::ChannelPending { .. })
        })
        .await;
        generate_blocks_and_wait(rpc, &electrsd, 6).await;
        payer.sync_wallets().expect("payer wallets should resync");
        receiver_handle
            .node
            .sync_wallets()
            .expect("receiver wallets should sync");
        wait_for_event(&payer, |event| matches!(event, Event::ChannelReady { .. })).await;
        wait_for_event(&receiver_handle.node, |event| {
            matches!(event, Event::ChannelReady { .. })
        })
        .await;

        let mut config = AppConfig::for_tests();
        config.lightning_backend = "ldk".into();
        config.onchain_backend = "mock".into();
        config.lightning_ldk_network = "regtest".into();
        config.lightning_ldk_esplora_url = esplora_url;
        config.lightning_ldk_rgs_url = None;

        let app = router_with_real_lightning(
            config.clone(),
            Arc::new(LdkNodeLightningAdapter::new(Arc::clone(&receiver_handle), 900)),
        );
        let seller_pubkey = test_public_key();
        let buyer_pubkey =
            "02cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();

        let quote_request = signed_envelope(json!({
            "buyer_nostr_pubkey": buyer_pubkey,
            "seller_nostr_pubkey": seller_pubkey,
            "callback_relays": ["wss://relay.damus.io"],
            "items": [
                {
                    "sku": "agent-plan",
                    "description": "Autonomous procurement plan",
                    "quantity": 1,
                    "unit_price_sats": 21000
                }
            ],
            "settlement_preference": "lightning_only",
            "buyer_reference": "ldk-live-flow"
        }));

        let (quote_status, quote_body) =
            json_request(app.clone(), Method::POST, "/quote", quote_request, None)
                .await
                .expect("quote request should succeed");
        assert_eq!(quote_status, 200);

        let quote_id = quote_body["quote_id"].as_str().expect("quote id");
        let order_id = quote_body["order_id"].as_str().expect("order id");
        let lightning_checkout = signed_envelope(json!({
            "quote_id": quote_id,
            "selected_rail": "lightning",
            "buyer_reference": "buyer-order-live",
            "return_relays": ["wss://nos.lol"]
        }));

        let (checkout_status, checkout_body) = json_request(
            app.clone(),
            Method::POST,
            "/checkout-intent",
            lightning_checkout,
            Some("checkout-live-1"),
        )
        .await
        .expect("checkout should succeed");
        if checkout_status != 200 {
            panic!("unexpected checkout response: {checkout_body}");
        }

        let payment_hash = checkout_body["lightning_payment_hash"]
            .as_str()
            .expect("payment hash")
            .to_owned();
        let invoice = checkout_body["lightning_invoice"]
            .as_str()
            .expect("bolt11 invoice");
        let parsed_invoice = Bolt11Invoice::from_str(invoice).expect("checkout invoice should parse");
        payer
            .bolt11_payment()
            .send(&parsed_invoice, None)
            .expect("payer should settle the checkout invoice");
        wait_for_event(&payer, |event| matches!(event, Event::PaymentSuccessful { .. })).await;
        wait_for_event(&receiver_handle.node, |event| {
            matches!(event, Event::PaymentReceived { .. })
        })
        .await;

        let (payment_status, payment_body) = wait_for_payment_confirmation(
            app.clone(),
            order_id,
            &payment_hash,
            21_000,
        )
        .await;
        assert_eq!(payment_status, 200);
        assert_eq!(payment_body["state"], "paid");

        let (order_status, order_body) = json_request(
            app,
            Method::GET,
            &format!("/order/{order_id}"),
            Value::Null,
            None,
        )
        .await
        .expect("order lookup should succeed");
        assert_eq!(order_status, 200);
        assert_eq!(order_body["state"], "paid");
        assert_eq!(order_body["quote_id"], quote_id);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires downloaded bitcoind/electrs binaries and a live regtest environment"]
    async fn saturn_router_completes_real_onchain_checkout() {
        let (bitcoind, electrsd) = setup_bitcoind_and_electrsd();
        let rpc = &bitcoind.client;
        let _ = rpc.create_wallet("saturn_router_onchain_regtest");
        let _ = rpc.load_wallet("saturn_router_onchain_regtest");
        generate_blocks_and_wait(rpc, &electrsd, 101).await;

        let esplora_url = format!(
            "http://{}",
            electrsd
                .esplora_url
                .as_ref()
                .expect("electrsd should expose esplora")
        );
        let receiver_handle = Arc::new(build_test_ldk_handle(&esplora_url, 71, None));

        let mut config = AppConfig::for_tests();
        config.lightning_backend = "mock".into();
        config.onchain_backend = "ldk".into();
        config.lightning_ldk_network = "regtest".into();
        config.lightning_ldk_esplora_url = esplora_url.clone();
        config.lightning_ldk_rgs_url = None;
        config.onchain_confirmations_required = 1;

        let app = router_with_real_onchain(
            config.clone(),
            Arc::new(LdkNodeOnChainAdapter::new(
                Arc::clone(&receiver_handle),
                esplora_url.clone(),
            )),
        );
        let seller_pubkey = test_public_key();
        let buyer_pubkey =
            "02dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_owned();

        let quote_request = signed_envelope(json!({
            "buyer_nostr_pubkey": buyer_pubkey,
            "seller_nostr_pubkey": seller_pubkey,
            "callback_relays": ["wss://relay.damus.io"],
            "items": [
                {
                    "sku": "agent-plan",
                    "description": "Autonomous procurement plan",
                    "quantity": 1,
                    "unit_price_sats": 21000
                }
            ],
            "settlement_preference": "lightning_with_onchain_fallback",
            "buyer_reference": "ldk-onchain-flow"
        }));

        let (quote_status, quote_body) =
            json_request(app.clone(), Method::POST, "/quote", quote_request, None)
                .await
                .expect("quote request should succeed");
        assert_eq!(quote_status, 200);

        let quote_id = quote_body["quote_id"].as_str().expect("quote id");
        let order_id = quote_body["order_id"].as_str().expect("order id");
        let checkout_request = signed_envelope(json!({
            "quote_id": quote_id,
            "selected_rail": "on_chain",
            "buyer_reference": "buyer-order-onchain",
            "return_relays": ["wss://nos.lol"]
        }));

        let (checkout_status, checkout_body) = json_request(
            app.clone(),
            Method::POST,
            "/checkout-intent",
            checkout_request,
            Some("checkout-onchain-1"),
        )
        .await
        .expect("checkout should succeed");
        if checkout_status != 200 {
            panic!("unexpected checkout response: {checkout_body}");
        }

        let onchain_address = checkout_body["onchain_fallback_address"]
            .as_str()
            .expect("checkout should return on-chain fallback address")
            .to_owned();
        let parsed_address = onchain_address
            .parse::<Address<NetworkUnchecked>>()
            .expect("fallback address should parse")
            .require_network(BitcoinNetwork::Regtest)
            .expect("fallback address should target regtest");
        let txid = rpc
            .send_to_address(&parsed_address, Amount::from_sat(21_000))
            .expect("bitcoind should fund the checkout address")
            .0
            .parse::<Txid>()
            .expect("checkout txid should parse");
        wait_for_esplora_tx(&esplora_url, &txid).await;
        let vout = find_output_index(&esplora_url, &txid, &onchain_address, 21_000)
            .await
            .expect("checkout tx should contain the expected output");

        let (pending_status, pending_body) =
            submit_onchain_payment_confirm(app.clone(), order_id, &txid.to_string(), vout, 21_000)
                .await;
        assert_eq!(pending_status, 202);
        assert_eq!(
            pending_body["error"]["code"],
            Value::String("payment_finality_pending".into())
        );

        generate_blocks_and_wait(rpc, &electrsd, 1).await;

        let (payment_status, payment_body) =
            submit_onchain_payment_confirm(app.clone(), order_id, &txid.to_string(), vout, 21_000)
                .await;
        assert_eq!(payment_status, 200);
        assert_eq!(payment_body["state"], "paid");

        let (order_status, order_body) = json_request(
            app,
            Method::GET,
            &format!("/order/{order_id}"),
            Value::Null,
            None,
        )
        .await
        .expect("order lookup should succeed");
        assert_eq!(order_status, 200);
        assert_eq!(order_body["state"], "paid");
        assert_eq!(order_body["quote_id"], quote_id);
    }

    fn setup_bitcoind_and_electrsd() -> (BitcoinD, ElectrsD) {
        let bitcoind_exe = env::var("BITCOIND_EXE")
            .ok()
            .or_else(|| corepc_node::downloaded_exe_path().ok())
            .expect("set BITCOIND_EXE or enable corepc-node downloads");
        let mut bitcoind_conf = corepc_node::Conf::default();
        bitcoind_conf.network = "regtest";
        bitcoind_conf.args.push("-rest");
        let bitcoind = BitcoinD::with_conf(bitcoind_exe, &bitcoind_conf)
            .expect("bitcoind should start for regtest");

        let electrs_exe = env::var("ELECTRS_EXE")
            .ok()
            .or_else(electrsd::downloaded_exe_path)
            .expect("set ELECTRS_EXE or enable electrsd downloads");
        let mut electrsd_conf = electrsd::Conf::default();
        electrsd_conf.http_enabled = true;
        electrsd_conf.network = "regtest";
        let electrsd = ElectrsD::with_conf(electrs_exe, &bitcoind, &electrsd_conf)
            .expect("electrsd should start for regtest");

        (bitcoind, electrsd)
    }

    fn build_test_ldk_node(
        esplora_url: &str,
        seed_byte: u8,
        listening_address: Option<SocketAddress>,
    ) -> LdkNode {
        let node = build_test_ldk_builder(esplora_url, seed_byte, listening_address)
            .build()
            .expect("test ldk node should build");
        node.start().expect("test ldk node should start");
        node
    }

    fn build_test_ldk_handle(
        esplora_url: &str,
        seed_byte: u8,
        listening_address: Option<SocketAddress>,
    ) -> LdkNodeHandle {
        let node = build_test_ldk_builder(esplora_url, seed_byte, listening_address)
            .build()
            .expect("test ldk handle should build");
        node.start().expect("test ldk handle should start");
        LdkNodeHandle { node }
    }

    fn build_test_ldk_builder(
        esplora_url: &str,
        seed_byte: u8,
        listening_address: Option<SocketAddress>,
    ) -> LdkNodeBuilder {
        let mut builder = LdkNodeBuilder::new();
        builder.set_network(BitcoinNetwork::Regtest);
        builder.set_entropy_seed_bytes([seed_byte; WALLET_KEYS_SEED_LEN]);
        builder.set_storage_dir_path(
            env::temp_dir()
                .join(format!("saturn-ldk-lightning-{}-{}", seed_byte, Uuid::new_v4()))
                .display()
                .to_string(),
        );
        builder.set_chain_source_esplora(esplora_url.to_owned(), None);
        if let Some(listening_address) = listening_address {
            builder
                .set_listening_addresses(vec![listening_address])
                .expect("listening address should be valid");
        }
        builder.set_log_facade_logger();
        builder
    }

    fn random_socket_address() -> SocketAddress {
        let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral listener");
        let address = listener.local_addr().expect("local addr");
        drop(listener);
        SocketAddress::from_str(&address.to_string()).expect("socket address should parse")
    }

    async fn generate_blocks_and_wait(
        rpc: &corepc_node::Client,
        electrsd: &ElectrsD,
        blocks: usize,
    ) {
        let start_height = rpc
            .get_blockchain_info()
            .expect("blockchain info should be available")
            .blocks as u32;
        let address = rpc
            .get_new_address(None, None)
            .expect("wallet address")
            .0
            .parse::<Address<NetworkUnchecked>>()
            .expect("mining address should parse")
            .assume_checked();
        rpc.generate_to_address(blocks, &address)
            .expect("regtest blocks should mine");

        let esplora_base_url = format!(
            "http://{}",
            electrsd
                .esplora_url
                .as_ref()
                .expect("electrsd should expose esplora")
        );
        wait_for_tip_height(&esplora_base_url, start_height.saturating_add(blocks as u32)).await;
    }

    async fn wait_for_tip_height(esplora_base_url: &str, target_height: u32) {
        let client = reqwest::Client::new();
        let url = format!("{}/blocks/tip/height", esplora_base_url);

        for _ in 0..120 {
            let response = client
                .get(&url)
                .send()
                .await
                .expect("tip height poll should succeed");
            let height: u32 = response
                .text()
                .await
                .expect("tip height body should be readable")
                .trim()
                .parse()
                .expect("tip height should parse");
            if height >= target_height {
                return;
            }
            sleep(Duration::from_millis(250)).await;
        }

        panic!("timed out waiting for esplora tip height to reach {target_height}");
    }

    async fn wait_for_esplora_tx(esplora_base_url: &str, txid: &Txid) {
        let client = reqwest::Client::new();
        let url = format!("{}/tx/{txid}", esplora_base_url);

        for _ in 0..120 {
            let response = client.get(&url).send().await.expect("tx poll should succeed");
            if response.status() == reqwest::StatusCode::OK {
                return;
            }
            sleep(Duration::from_millis(250)).await;
        }

        panic!("timed out waiting for esplora to index transaction {txid}");
    }

    async fn find_output_index(
        esplora_base_url: &str,
        txid: &Txid,
        expected_address: &str,
        expected_value: u64,
    ) -> Option<u32> {
        let client = reqwest::Client::new();
        let url = format!("{}/tx/{txid}", esplora_base_url);
        let transaction: EsploraTransaction = client
            .get(&url)
            .send()
            .await
            .expect("transaction query should succeed")
            .json()
            .await
            .expect("transaction response should decode");

        transaction
            .vout
            .iter()
            .enumerate()
            .find(|(_, output)| {
                output.scriptpubkey_address.as_deref() == Some(expected_address)
                    && output.value == expected_value
            })
            .map(|(index, _)| index as u32)
    }

    async fn wait_for_event<F>(node: &LdkNode, matcher: F)
    where
        F: Fn(&Event) -> bool,
    {
        for _ in 0..120 {
            let event = node.next_event_async().await;
            if matcher(&event) {
                node.event_handled().expect("event should be acknowledged");
                return;
            }
            node.event_handled().expect("event should be acknowledged");
        }

        panic!("timed out waiting for expected LDK event");
    }

    async fn wait_for_lightning_verification(
        adapter: &LdkNodeLightningAdapter,
        proof: &SettlementProof,
        expected_hash: Option<&str>,
        expected_amount_sats: i64,
    ) -> PaymentVerification {
        for _ in 0..120 {
            match adapter
                .verify_payment(proof, expected_hash, expected_amount_sats)
                .await
            {
                Ok(verification) => return verification,
                Err(error) if error.message.contains("still pending") => {
                    sleep(Duration::from_millis(250)).await;
                }
                Err(error) => panic!("lightning verification should succeed: {}", error.message),
            }
        }

        panic!("timed out waiting for settled lightning payment");
    }

    fn router_with_real_lightning(
        config: AppConfig,
        lightning_adapter: DynLightningAdapter,
    ) -> Router {
        let relays = config.nostr_relays.clone();
        build_router(AppState::new(
            config,
            Arc::new(InMemoryQuoteRepository::default()),
            Arc::new(InMemoryOrderRepository::default()),
            Arc::new(InMemoryReceiptRepository::default()),
            Arc::new(InMemoryNonceRepository::default()),
            lightning_adapter,
            Arc::new(MockOnChainAdapter),
            Arc::new(MockNostrPublisher::new(relays)),
            Arc::new(DisabledCoinjoinClient),
        ))
    }

    fn router_with_real_onchain(config: AppConfig, onchain_adapter: DynOnChainAdapter) -> Router {
        let relays = config.nostr_relays.clone();
        build_router(AppState::new(
            config,
            Arc::new(InMemoryQuoteRepository::default()),
            Arc::new(InMemoryOrderRepository::default()),
            Arc::new(InMemoryReceiptRepository::default()),
            Arc::new(InMemoryNonceRepository::default()),
            Arc::new(MockLightningAdapter),
            onchain_adapter,
            Arc::new(MockNostrPublisher::new(relays)),
            Arc::new(DisabledCoinjoinClient),
        ))
    }

    fn test_secret_key() -> &'static str {
        "1111111111111111111111111111111111111111111111111111111111111111"
    }

    fn test_public_key() -> String {
        derive_public_key(test_secret_key()).expect("public key derivation should work")
    }

    fn signed_envelope(payload: Value) -> Value {
        let mut object = payload
            .as_object()
            .cloned()
            .expect("payload must be an object");
        object.insert("message_id".into(), json!(Uuid::new_v4()));
        object.insert("timestamp".into(), json!(Utc::now()));
        object.insert("nonce".into(), json!(format!("nonce-{}", Uuid::new_v4())));
        object.insert("public_key".into(), json!(test_public_key()));
        object.insert("signature".into(), json!(""));
        signed_payload(Value::Object(object))
    }

    fn signed_payload(mut payload: Value) -> Value {
        payload["public_key"] = Value::String(test_public_key());
        payload["signature"] = Value::String(String::new());
        sign_value(&mut payload, test_secret_key()).expect("payload signing should work");
        payload
    }

    async fn json_request(
        app: Router,
        method: Method,
        path: &str,
        body: Value,
        idempotency_key: Option<&str>,
    ) -> AppResult<(u16, Value)> {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if let Some(idempotency_key) = idempotency_key {
            builder = builder.header("Idempotency-Key", idempotency_key);
        }

        let response = app
            .oneshot(
                builder
                    .body(Body::from(body.to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");
        let status = response.status().as_u16();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("response should be valid json")
        };
        Ok((status, value))
    }

    async fn wait_for_payment_confirmation(
        app: Router,
        order_id: &str,
        payment_hash: &str,
        amount_sats: i64,
    ) -> (u16, Value) {
        for _ in 0..120 {
            let envelope = signed_envelope(json!({
                "order_id": order_id,
                "rail": "lightning",
                "settlement_proof": {
                    "type": "lightning",
                    "payment_hash": payment_hash,
                    "preimage": Value::Null,
                    "settled_at": Utc::now(),
                    "amount_sats": amount_sats
                }
            }));

            let (status, body) = json_request(
                app.clone(),
                Method::POST,
                "/payment/confirm",
                envelope,
                Some("payment-live-1"),
            )
            .await
            .expect("payment confirm request should complete");
            if status == 200 {
                return (status, body);
            }

            let message = body["error"]["message"].as_str().unwrap_or_default();
            if message.contains("still pending") || message.contains("has no record") {
                sleep(Duration::from_millis(250)).await;
                continue;
            }

            panic!("unexpected payment confirmation failure: {body}");
        }

        panic!("timed out waiting for payment confirmation to succeed");
    }

    async fn submit_onchain_payment_confirm(
        app: Router,
        order_id: &str,
        txid: &str,
        vout: u32,
        amount_sats: i64,
    ) -> (u16, Value) {
        let envelope = signed_envelope(json!({
            "order_id": order_id,
            "rail": "on_chain",
            "settlement_proof": {
                "type": "on_chain",
                "txid": txid,
                "vout": vout,
                "amount_sats": amount_sats,
                "confirmations": 0
            }
        }));

        json_request(
            app,
            Method::POST,
            "/payment/confirm",
            envelope,
            Some("payment-onchain-1"),
        )
        .await
        .expect("on-chain payment confirm request should complete")
    }
}
