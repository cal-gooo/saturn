use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use tracing::info;
use url::Url;
use uuid::Uuid;

use crate::{
    app::config::AppConfig,
    domain::entities::{Order, SettlementProof},
    errors::{ApiError, AppResult},
};

pub type DynCoinjoinClient = Arc<dyn CoinjoinClient>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoinjoinCandidate {
    pub order_id: Uuid,
    pub merchant_nostr_pubkey: String,
    pub network: String,
    pub address: String,
    pub txid: String,
    pub vout: u32,
    pub amount_sats: i64,
    pub confirmations: u32,
    pub receipt_event_id: Option<String>,
    pub queued_at: DateTime<Utc>,
}

impl CoinjoinCandidate {
    pub fn from_confirmed_onchain_order(
        order: &Order,
        merchant_nostr_pubkey: &str,
        network: &str,
        receipt_event_id: Option<&str>,
    ) -> AppResult<Self> {
        let address = order
            .onchain_address
            .clone()
            .ok_or_else(|| ApiError::internal("missing on-chain address for coinjoin candidate"))?;
        let proof = order
            .settlement_proof
            .as_ref()
            .ok_or_else(|| ApiError::internal("missing settlement proof for coinjoin candidate"))?;

        match proof {
            SettlementProof::OnChain {
                txid,
                vout,
                amount_sats,
                confirmations,
            } => Ok(Self {
                order_id: order.id,
                merchant_nostr_pubkey: merchant_nostr_pubkey.to_owned(),
                network: network.to_owned(),
                address,
                txid: txid.clone(),
                vout: *vout,
                amount_sats: *amount_sats,
                confirmations: *confirmations,
                receipt_event_id: receipt_event_id.map(str::to_owned),
                queued_at: Utc::now(),
            }),
            SettlementProof::Lightning { .. } => Err(ApiError::internal(
                "cannot create coinjoin candidate from lightning settlement proof",
            )),
        }
    }
}

#[async_trait]
pub trait CoinjoinClient: Send + Sync {
    async fn enqueue_confirmed_output(&self, candidate: &CoinjoinCandidate) -> AppResult<()>;

    fn enabled(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Default)]
pub struct DisabledCoinjoinClient;

#[async_trait]
impl CoinjoinClient for DisabledCoinjoinClient {
    async fn enqueue_confirmed_output(&self, _candidate: &CoinjoinCandidate) -> AppResult<()> {
        Ok(())
    }

    fn enabled(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone)]
pub struct JoinstrSidecarClient {
    client: Client,
    enqueue_url: Url,
    api_token: Option<String>,
}

impl JoinstrSidecarClient {
    pub fn new(config: &AppConfig) -> AppResult<Self> {
        let enqueue_url = config.joinstr_sidecar_url.clone().ok_or_else(|| {
            ApiError::internal(
                "APP__JOINSTR_SIDECAR_URL is required when APP__COINJOIN_BACKEND=joinstr_sidecar",
            )
        })?;
        let client = Client::builder()
            .timeout(Duration::from_secs(config.joinstr_sidecar_timeout_seconds))
            .build()
            .map_err(|error| {
                ApiError::internal(format!("failed to build Joinstr sidecar client: {error}"))
            })?;

        Ok(Self {
            client,
            enqueue_url,
            api_token: config.joinstr_sidecar_api_token.clone(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct JoinstrEnqueueResponse {
    request_id: Option<String>,
    round_id: Option<String>,
    status: Option<String>,
}

#[async_trait]
impl CoinjoinClient for JoinstrSidecarClient {
    async fn enqueue_confirmed_output(&self, candidate: &CoinjoinCandidate) -> AppResult<()> {
        let mut request = self.client.post(self.enqueue_url.clone()).json(candidate);
        if let Some(token) = &self.api_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|error| {
            ApiError::internal(format!("failed to reach Joinstr sidecar: {error}"))
        })?;
        let status = response.status();

        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unavailable>".to_owned());
            return Err(ApiError::internal(format!(
                "Joinstr sidecar rejected coinjoin candidate with HTTP {status}: {body}"
            )));
        }

        let payload = if status == StatusCode::NO_CONTENT {
            None
        } else {
            response.json::<JoinstrEnqueueResponse>().await.ok()
        };

        info!(
            order_id = %candidate.order_id,
            request_id = payload.as_ref().and_then(|value| value.request_id.as_deref()),
            round_id = payload.as_ref().and_then(|value| value.round_id.as_deref()),
            status = payload.as_ref().and_then(|value| value.status.as_deref()),
            "queued confirmed on-chain output for Joinstr sidecar"
        );

        Ok(())
    }
}

pub fn build_coinjoin_client(config: &AppConfig) -> AppResult<DynCoinjoinClient> {
    match config.coinjoin_backend.as_str() {
        "disabled" | "mock" => Ok(Arc::new(DisabledCoinjoinClient)),
        "joinstr_sidecar" => Ok(Arc::new(JoinstrSidecarClient::new(config)?)),
        other => Err(ApiError::internal(format!(
            "unsupported coinjoin backend: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{CoinjoinCandidate, build_coinjoin_client};
    use crate::{
        app::config::AppConfig,
        domain::entities::{Order, OrderState, SettlementProof},
    };
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn builds_candidate_from_confirmed_onchain_order() {
        let order = Order {
            id: Uuid::new_v4(),
            quote_id: Uuid::new_v4(),
            buyer_pubkey: "buyer".into(),
            seller_pubkey: "seller".into(),
            state: OrderState::Paid,
            selected_rail: None,
            checkout_idempotency_key: None,
            payment_confirm_idempotency_key: None,
            lightning_invoice: None,
            lightning_payment_hash: None,
            onchain_address: Some("bcrt1qexampleaddress".into()),
            payment_amount_sats: Some(21_000),
            settlement_proof: Some(SettlementProof::OnChain {
                txid: "ab".repeat(32),
                vout: 1,
                amount_sats: 21_000,
                confirmations: 6,
            }),
            onchain_confirmations: Some(6),
            last_error_code: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let candidate = CoinjoinCandidate::from_confirmed_onchain_order(
            &order,
            "merchant-pubkey",
            "regtest",
            Some("receipt-event"),
        )
        .expect("candidate should build");

        assert_eq!(candidate.order_id, order.id);
        assert_eq!(candidate.address, "bcrt1qexampleaddress");
        assert_eq!(candidate.txid, "ab".repeat(32));
        assert_eq!(candidate.vout, 1);
        assert_eq!(candidate.amount_sats, 21_000);
        assert_eq!(candidate.confirmations, 6);
        assert_eq!(candidate.receipt_event_id.as_deref(), Some("receipt-event"));
    }

    #[test]
    fn rejects_unknown_coinjoin_backend() {
        let mut config = AppConfig::for_tests();
        config.coinjoin_backend = "unknown".into();

        let error = match build_coinjoin_client(&config) {
            Ok(_) => panic!("unknown backend should fail"),
            Err(error) => error,
        };

        assert!(
            error.message.contains("unsupported coinjoin backend"),
            "unexpected error: {}",
            error.message
        );
    }
}
