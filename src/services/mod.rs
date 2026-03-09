use chrono::Utc;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    api::schemas::{
        CapabilitiesResponse, CheckoutIntentPayload, CheckoutIntentResponse, FulfillOrderPayload,
        OrderResponse, PaymentConfirmPayload, PaymentConfirmResponse, QuoteRequestPayload,
        QuoteResponse, SettlementProofInput, SignedEnvelope,
    },
    app::AppState,
    domain::{
        entities::{
            Order, OrderState, PaymentFinality, PaymentRail, Quote, Receipt, SettlementProof,
            total_sats,
        },
        state_machine::ensure_transition,
    },
    errors::{ApiError, AppResult},
    nostr::experimental_event_kinds,
    payments::{build_receipt_payload, receipt_hash},
    privacy::CoinjoinCandidate,
};

pub struct CapabilityService {
    state: AppState,
}

impl CapabilityService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn get_capabilities(&self) -> AppResult<CapabilitiesResponse> {
        let _ = self
            .state
            .nostr_publisher
            .publish_capability(&self.state.config)
            .await?;

        Ok(CapabilitiesResponse {
            protocol: "a2ac".into(),
            version: "0.1.0".into(),
            merchant_name: self.state.config.merchant_name.clone(),
            merchant_nostr_pubkey: self.state.config.merchant_nostr_pubkey.clone(),
            relay_urls: self.state.config.nostr_relays.clone(),
            supported_rails: vec![PaymentRail::Lightning, PaymentRail::OnChain],
            quote_ttl_seconds: self.state.config.quote_ttl_seconds,
            quote_lock_seconds: self.state.config.quote_lock_seconds,
            onchain_confirmations_required: self.state.config.onchain_confirmations_required,
            experimental_event_kinds: experimental_event_kinds(),
        })
    }
}

pub struct QuoteService {
    state: AppState,
}

impl QuoteService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    #[instrument(skip_all, fields(buyer = %envelope.payload.buyer_nostr_pubkey))]
    pub async fn create_quote(
        &self,
        envelope: SignedEnvelope<QuoteRequestPayload>,
    ) -> AppResult<QuoteResponse> {
        let now = Utc::now();
        let quote_id = Uuid::new_v4();
        let order_id = Uuid::new_v4();
        let expires_at =
            now + chrono::Duration::seconds(self.state.config.quote_ttl_seconds as i64);
        let quote_lock_until =
            now + chrono::Duration::seconds(self.state.config.quote_lock_seconds as i64);
        let accepted_rails = envelope.payload.settlement_preference.accepted_rails();
        let total_sats = total_sats(&envelope.payload.items);

        let mut order = Order {
            id: order_id,
            quote_id,
            buyer_pubkey: envelope.payload.buyer_nostr_pubkey.clone(),
            seller_pubkey: envelope.payload.seller_nostr_pubkey.clone(),
            state: OrderState::Created,
            selected_rail: None,
            checkout_idempotency_key: None,
            payment_confirm_idempotency_key: None,
            lightning_invoice: None,
            lightning_payment_hash: None,
            onchain_address: None,
            payment_amount_sats: None,
            settlement_proof: None,
            onchain_confirmations: None,
            last_error_code: None,
            created_at: now,
            updated_at: now,
        };
        ensure_transition(order.state, OrderState::Quoted)?;
        order.state = OrderState::Quoted;

        let quote = Quote {
            id: quote_id,
            order_id,
            buyer_pubkey: envelope.payload.buyer_nostr_pubkey.clone(),
            seller_pubkey: envelope.payload.seller_nostr_pubkey.clone(),
            items: envelope.payload.items.clone(),
            settlement_preference: envelope.payload.settlement_preference,
            callback_relays: envelope.payload.callback_relays.clone(),
            buyer_reference: envelope.payload.buyer_reference.clone(),
            total_sats,
            status: OrderState::Quoted,
            expires_at,
            quote_lock_until,
            accepted_rails: accepted_rails.clone(),
            created_at: now,
            updated_at: now,
        };

        self.state.quote_repository.insert_quote(&quote).await?;
        self.state.order_repository.insert_order(&order).await?;

        let nostr_quote_reference = self
            .state
            .nostr_publisher
            .publish_quote_reference(&quote)
            .await?;

        info!(quote_id = %quote.id, order_id = %order.id, "quote created");

        Ok(QuoteResponse {
            quote_id: quote.id,
            order_id: quote.order_id,
            state: quote.status,
            total_sats: quote.total_sats,
            expires_at: quote.expires_at,
            quote_lock_until: quote.quote_lock_until,
            accepted_rails,
            nostr_quote_reference,
        })
    }
}

pub struct CheckoutService {
    state: AppState,
}

impl CheckoutService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    #[instrument(skip_all, fields(quote_id = %envelope.payload.quote_id))]
    pub async fn create_checkout_intent(
        &self,
        envelope: SignedEnvelope<CheckoutIntentPayload>,
        idempotency_key: String,
    ) -> AppResult<CheckoutIntentResponse> {
        let quote = self
            .state
            .quote_repository
            .get_quote(envelope.payload.quote_id)
            .await?
            .ok_or_else(|| ApiError::resource_not_found("quote"))?;

        if quote.expires_at < Utc::now() || quote.quote_lock_until < Utc::now() {
            return Err(ApiError::quote_expired());
        }

        let mut order = self
            .state
            .order_repository
            .get_order_by_quote_id(quote.id)
            .await?
            .ok_or_else(|| ApiError::resource_not_found("order"))?;

        if let Some(existing) = &order.checkout_idempotency_key {
            if existing == &idempotency_key && order.state == OrderState::PaymentPending {
                return Ok(CheckoutIntentResponse {
                    order_id: order.id,
                    quote_id: order.quote_id,
                    state: order.state,
                    selected_rail: order
                        .selected_rail
                        .unwrap_or(envelope.payload.selected_rail),
                    lightning_invoice: order
                        .lightning_invoice
                        .clone()
                        .ok_or_else(|| ApiError::internal("missing cached lightning invoice"))?,
                    lightning_payment_hash: order.lightning_payment_hash.clone().ok_or_else(
                        || ApiError::internal("missing cached lightning payment hash"),
                    )?,
                    onchain_fallback_address: order.onchain_address.clone(),
                    quote_lock_until: quote.quote_lock_until,
                    required_onchain_confirmations: self
                        .state
                        .config
                        .onchain_confirmations_required,
                });
            }
            return Err(ApiError::idempotency_conflict());
        }

        ensure_transition(order.state, OrderState::PaymentPending)?;

        let invoice = self
            .state
            .lightning_adapter
            .create_invoice(order.id, quote.total_sats, "a2ac checkout")
            .await?;
        let onchain_fallback_address = if quote.accepted_rails.contains(&PaymentRail::OnChain) {
            Some(self.state.onchain_adapter.new_address(order.id).await?)
        } else {
            None
        };

        order.state = OrderState::PaymentPending;
        order.selected_rail = Some(envelope.payload.selected_rail);
        order.checkout_idempotency_key = Some(idempotency_key);
        order.lightning_invoice = Some(invoice.bolt11.clone());
        order.lightning_payment_hash = Some(invoice.payment_hash.clone());
        order.onchain_address = onchain_fallback_address.clone();
        order.payment_amount_sats = Some(quote.total_sats);
        order.updated_at = Utc::now();

        self.state.order_repository.update_order(&order).await?;

        Ok(CheckoutIntentResponse {
            order_id: order.id,
            quote_id: order.quote_id,
            state: order.state,
            selected_rail: order
                .selected_rail
                .ok_or_else(|| ApiError::internal("missing selected rail"))?,
            lightning_invoice: invoice.bolt11,
            lightning_payment_hash: invoice.payment_hash,
            onchain_fallback_address,
            quote_lock_until: quote.quote_lock_until,
            required_onchain_confirmations: self.state.config.onchain_confirmations_required,
        })
    }
}

pub struct PaymentService {
    state: AppState,
}

impl PaymentService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    #[instrument(skip_all, fields(order_id = %envelope.payload.order_id))]
    pub async fn confirm_payment(
        &self,
        envelope: SignedEnvelope<PaymentConfirmPayload>,
        idempotency_key: String,
    ) -> AppResult<PaymentConfirmResponse> {
        let mut order = self
            .state
            .order_repository
            .get_order(envelope.payload.order_id)
            .await?
            .ok_or_else(|| ApiError::resource_not_found("order"))?;

        if let Some(existing) = &order.payment_confirm_idempotency_key {
            if existing == &idempotency_key && order.state == OrderState::Paid {
                let receipt = self
                    .state
                    .receipt_repository
                    .get_receipt_by_order_id(order.id)
                    .await?
                    .ok_or_else(|| ApiError::internal("missing cached receipt"))?;
                return Ok(PaymentConfirmResponse {
                    order_id: order.id,
                    receipt_id: receipt.id,
                    state: order.state,
                    finality: receipt.finality,
                    receipt_event_id: receipt.nostr_event_id,
                });
            }
            return Err(ApiError::idempotency_conflict());
        }

        ensure_transition(order.state, OrderState::Paid)?;
        let expected_amount = order
            .payment_amount_sats
            .ok_or_else(|| ApiError::internal("missing expected payment amount"))?;
        let proof = settlement_input_into_domain(envelope.payload.settlement_proof);

        let verification = match envelope.payload.rail {
            PaymentRail::Lightning => {
                self.state
                    .lightning_adapter
                    .verify_payment(
                        &proof,
                        order.lightning_payment_hash.as_deref(),
                        expected_amount,
                    )
                    .await?
            }
            PaymentRail::OnChain => {
                let expected_address = order
                    .onchain_address
                    .as_deref()
                    .ok_or_else(|| ApiError::internal("missing expected on-chain address"))?;
                self.state
                    .onchain_adapter
                    .verify_settlement(
                        &proof,
                        expected_address,
                        expected_amount,
                        self.state.config.onchain_confirmations_required,
                    )
                    .await?
            }
        };

        if verification.finality == PaymentFinality::Pending {
            let confirmations = match verification.normalized_proof {
                SettlementProof::OnChain { confirmations, .. } => confirmations,
                _ => 0,
            };
            return Err(ApiError::payment_finality_pending(
                confirmations,
                self.state.config.onchain_confirmations_required,
            ));
        }

        order.state = OrderState::Paid;
        order.payment_confirm_idempotency_key = Some(idempotency_key);
        order.settlement_proof = Some(verification.normalized_proof.clone());
        order.onchain_confirmations = match verification.normalized_proof {
            SettlementProof::OnChain { confirmations, .. } => Some(confirmations),
            _ => None,
        };
        order.updated_at = Utc::now();

        self.state.order_repository.update_order(&order).await?;

        let receipt_payload = build_receipt_payload(
            order.id,
            &envelope.payload.rail.to_string(),
            expected_amount,
            &verification.finality,
            verification.settled_at,
        );
        let mut receipt = Receipt {
            id: Uuid::new_v4(),
            order_id: order.id,
            rail: envelope.payload.rail,
            receipt_hash: receipt_hash(&receipt_payload),
            nostr_event_id: None,
            finality: verification.finality,
            payload: receipt_payload,
            created_at: Utc::now(),
        };

        let nostr_ref = self
            .state
            .nostr_publisher
            .publish_receipt(&order, &receipt, &self.state.config.merchant_nostr_pubkey)
            .await?;
        receipt.nostr_event_id = Some(nostr_ref.event_id.clone());
        self.state
            .receipt_repository
            .insert_receipt(&receipt)
            .await?;

        if envelope.payload.rail == PaymentRail::OnChain {
            maybe_enqueue_coinjoin_candidate(&self.state, &order, &receipt).await;
        }

        Ok(PaymentConfirmResponse {
            order_id: order.id,
            receipt_id: receipt.id,
            state: order.state,
            finality: receipt.finality,
            receipt_event_id: receipt.nostr_event_id,
        })
    }
}

pub struct OrderService {
    state: AppState,
}

impl OrderService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn get_order(&self, order_id: Uuid) -> AppResult<OrderResponse> {
        let order = self
            .state
            .order_repository
            .get_order(order_id)
            .await?
            .ok_or_else(|| ApiError::resource_not_found("order"))?;
        self.order_response_from_order(order).await
    }

    #[instrument(skip_all, fields(order_id = %envelope.payload.order_id))]
    pub async fn fulfill_order(
        &self,
        envelope: SignedEnvelope<FulfillOrderPayload>,
    ) -> AppResult<OrderResponse> {
        let mut order = self
            .state
            .order_repository
            .get_order(envelope.payload.order_id)
            .await?
            .ok_or_else(|| ApiError::resource_not_found("order"))?;
        ensure_transition(order.state, OrderState::Fulfilled)?;
        order.state = OrderState::Fulfilled;
        order.updated_at = Utc::now();
        self.state.order_repository.update_order(&order).await?;
        info!(order_id = %order.id, "order marked fulfilled");
        self.order_response_from_order(order).await
    }

    async fn order_response_from_order(&self, order: Order) -> AppResult<OrderResponse> {
        let receipt = self
            .state
            .receipt_repository
            .get_receipt_by_order_id(order.id)
            .await?;

        Ok(OrderResponse {
            order_id: order.id,
            quote_id: order.quote_id,
            state: order.state,
            selected_rail: order.selected_rail,
            payment_amount_sats: order.payment_amount_sats,
            receipt_ids: receipt.into_iter().map(|value| value.id).collect(),
        })
    }
}

fn settlement_input_into_domain(input: SettlementProofInput) -> SettlementProof {
    match input {
        SettlementProofInput::Lightning {
            payment_hash,
            preimage,
            settled_at,
            amount_sats,
        } => SettlementProof::Lightning {
            payment_hash,
            preimage,
            settled_at,
            amount_sats,
        },
        SettlementProofInput::OnChain {
            txid,
            vout,
            amount_sats,
            confirmations,
        } => SettlementProof::OnChain {
            txid,
            vout,
            amount_sats,
            confirmations,
        },
    }
}

async fn maybe_enqueue_coinjoin_candidate(state: &AppState, order: &Order, receipt: &Receipt) {
    if !state.coinjoin_client.enabled() {
        return;
    }

    let candidate = match CoinjoinCandidate::from_confirmed_onchain_order(
        order,
        &state.config.merchant_nostr_pubkey,
        &state.config.lightning_ldk_network,
        receipt.nostr_event_id.as_deref(),
    ) {
        Ok(candidate) => candidate,
        Err(error) => {
            warn!(
                order_id = %order.id,
                error = %error.message,
                "skipping Joinstr enqueue because the on-chain settlement could not be normalized"
            );
            return;
        }
    };

    if let Err(error) = state
        .coinjoin_client
        .enqueue_confirmed_output(&candidate)
        .await
    {
        warn!(
            order_id = %order.id,
            error = %error.message,
            "failed to enqueue confirmed on-chain output for Joinstr sidecar"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use chrono::Utc;
    use uuid::Uuid;

    use super::PaymentService;
    use crate::{
        api::schemas::{PaymentConfirmPayload, SettlementProofInput, SignedEnvelope},
        app::AppState,
        domain::entities::{Order, OrderState, PaymentRail},
        errors::{ApiError, AppResult},
        privacy::{CoinjoinCandidate, CoinjoinClient},
    };

    #[derive(Debug, Default)]
    struct RecordingCoinjoinClient {
        candidates: Mutex<Vec<CoinjoinCandidate>>,
        fail: bool,
    }

    #[async_trait]
    impl CoinjoinClient for RecordingCoinjoinClient {
        async fn enqueue_confirmed_output(&self, candidate: &CoinjoinCandidate) -> AppResult<()> {
            self.candidates
                .lock()
                .expect("coinjoin client lock should work")
                .push(candidate.clone());
            if self.fail {
                Err(ApiError::internal("sidecar unavailable"))
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn confirmed_onchain_payment_enqueues_coinjoin_candidate() {
        let mut state = AppState::for_tests();
        let client = Arc::new(RecordingCoinjoinClient::default());
        state.coinjoin_client = client.clone();

        let order = pending_onchain_order();
        state
            .order_repository
            .insert_order(&order)
            .await
            .expect("order insert should succeed");

        let response = PaymentService::new(state.clone())
            .confirm_payment(confirmed_onchain_envelope(order.id), "confirm-1".into())
            .await
            .expect("payment confirm should succeed");

        assert_eq!(response.state, OrderState::Paid);

        let candidates = client
            .candidates
            .lock()
            .expect("coinjoin client lock should work");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].order_id, order.id);
        assert_eq!(candidates[0].amount_sats, 21_000);
        assert_eq!(candidates[0].confirmations, 6);
    }

    #[tokio::test]
    async fn coinjoin_sidecar_failure_does_not_fail_payment_confirmation() {
        let mut state = AppState::for_tests();
        let client = Arc::new(RecordingCoinjoinClient {
            candidates: Mutex::new(Vec::new()),
            fail: true,
        });
        state.coinjoin_client = client.clone();

        let order = pending_onchain_order();
        state
            .order_repository
            .insert_order(&order)
            .await
            .expect("order insert should succeed");

        let response = PaymentService::new(state.clone())
            .confirm_payment(confirmed_onchain_envelope(order.id), "confirm-2".into())
            .await
            .expect("payment confirm should still succeed");

        assert_eq!(response.state, OrderState::Paid);
        assert_eq!(
            response.finality,
            crate::domain::entities::PaymentFinality::Confirmed
        );

        let candidates = client
            .candidates
            .lock()
            .expect("coinjoin client lock should work");
        assert_eq!(candidates.len(), 1);
    }

    #[tokio::test]
    async fn lightning_payment_does_not_enqueue_coinjoin_candidate() {
        let mut state = AppState::for_tests();
        let client = Arc::new(RecordingCoinjoinClient::default());
        state.coinjoin_client = client.clone();

        let order = pending_lightning_order();
        state
            .order_repository
            .insert_order(&order)
            .await
            .expect("order insert should succeed");

        let response = PaymentService::new(state.clone())
            .confirm_payment(settled_lightning_envelope(order.id), "confirm-3".into())
            .await
            .expect("payment confirm should succeed");

        assert_eq!(response.state, OrderState::Paid);
        assert!(
            client
                .candidates
                .lock()
                .expect("coinjoin client lock should work")
                .is_empty(),
            "lightning settlements should not be sent to Joinstr"
        );
    }

    fn pending_onchain_order() -> Order {
        Order {
            id: Uuid::new_v4(),
            quote_id: Uuid::new_v4(),
            buyer_pubkey: "buyer".into(),
            seller_pubkey: "seller".into(),
            state: OrderState::PaymentPending,
            selected_rail: Some(PaymentRail::OnChain),
            checkout_idempotency_key: Some("checkout-1".into()),
            payment_confirm_idempotency_key: None,
            lightning_invoice: Some("lnbc21000n1test".into()),
            lightning_payment_hash: Some("ab".repeat(32)),
            onchain_address: Some("bcrt1qexampleaddress".into()),
            payment_amount_sats: Some(21_000),
            settlement_proof: None,
            onchain_confirmations: None,
            last_error_code: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn pending_lightning_order() -> Order {
        Order {
            id: Uuid::new_v4(),
            quote_id: Uuid::new_v4(),
            buyer_pubkey: "buyer".into(),
            seller_pubkey: "seller".into(),
            state: OrderState::PaymentPending,
            selected_rail: Some(PaymentRail::Lightning),
            checkout_idempotency_key: Some("checkout-2".into()),
            payment_confirm_idempotency_key: None,
            lightning_invoice: Some("lnbc21000n1test".into()),
            lightning_payment_hash: Some("cd".repeat(32)),
            onchain_address: None,
            payment_amount_sats: Some(21_000),
            settlement_proof: None,
            onchain_confirmations: None,
            last_error_code: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn confirmed_onchain_envelope(order_id: Uuid) -> SignedEnvelope<PaymentConfirmPayload> {
        SignedEnvelope {
            message_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            nonce: "nonce-onchain".into(),
            public_key: "pubkey".into(),
            signature: "signature".into(),
            payload: PaymentConfirmPayload {
                order_id,
                rail: PaymentRail::OnChain,
                settlement_proof: SettlementProofInput::OnChain {
                    txid: "ab".repeat(32),
                    vout: 1,
                    amount_sats: 21_000,
                    confirmations: 6,
                },
            },
        }
    }

    fn settled_lightning_envelope(order_id: Uuid) -> SignedEnvelope<PaymentConfirmPayload> {
        SignedEnvelope {
            message_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            nonce: "nonce-lightning".into(),
            public_key: "pubkey".into(),
            signature: "signature".into(),
            payload: PaymentConfirmPayload {
                order_id,
                rail: PaymentRail::Lightning,
                settlement_proof: SettlementProofInput::Lightning {
                    payment_hash: "cd".repeat(32),
                    preimage: Some("preimage".into()),
                    settled_at: Utc::now(),
                    amount_sats: 21_000,
                },
            },
        }
    }
}
