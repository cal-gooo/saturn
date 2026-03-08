use axum::{
    Extension, Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    api::schemas::{
        CapabilitiesResponse, CheckoutIntentPayload, CheckoutIntentResponse, OrderResponse,
        PaymentConfirmPayload, PaymentConfirmResponse, QuoteRequestPayload, QuoteResponse,
        SignedEnvelope, parse_json, validate_payload,
    },
    app::AppState,
    errors::{ApiError, AppResult},
    security::VerifiedRequestContext,
    services::{CapabilityService, CheckoutService, OrderService, PaymentService, QuoteService},
};

pub async fn get_capabilities(
    State(state): State<AppState>,
) -> AppResult<Json<CapabilitiesResponse>> {
    let response = CapabilityService::new(state).get_capabilities().await?;
    Ok(Json(response))
}

pub async fn post_quote(
    State(state): State<AppState>,
    Extension(_verified): Extension<VerifiedRequestContext>,
    Json(raw): Json<Value>,
) -> AppResult<Json<QuoteResponse>> {
    let envelope: SignedEnvelope<QuoteRequestPayload> = parse_json(raw)?;
    validate_payload(&envelope.payload)?;
    let response = QuoteService::new(state).create_quote(envelope).await?;
    Ok(Json(response))
}

pub async fn post_checkout_intent(
    State(state): State<AppState>,
    Extension(_verified): Extension<VerifiedRequestContext>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> AppResult<Json<CheckoutIntentResponse>> {
    let idempotency_key = headers
        .get("Idempotency-Key")
        .or_else(|| headers.get("idempotency-key"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(ApiError::idempotency_missing)?;

    let envelope: SignedEnvelope<CheckoutIntentPayload> = parse_json(raw)?;
    validate_payload(&envelope.payload)?;

    let response = CheckoutService::new(state)
        .create_checkout_intent(envelope, idempotency_key)
        .await?;
    Ok(Json(response))
}

pub async fn post_payment_confirm(
    State(state): State<AppState>,
    Extension(_verified): Extension<VerifiedRequestContext>,
    headers: HeaderMap,
    Json(raw): Json<Value>,
) -> AppResult<Json<PaymentConfirmResponse>> {
    let idempotency_key = headers
        .get("Idempotency-Key")
        .or_else(|| headers.get("idempotency-key"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(ApiError::idempotency_missing)?;

    let envelope: SignedEnvelope<PaymentConfirmPayload> = parse_json(raw)?;
    validate_payload(&envelope.payload)?;

    let response = PaymentService::new(state)
        .confirm_payment(envelope, idempotency_key)
        .await?;
    Ok(Json(response))
}

pub async fn get_order(
    State(state): State<AppState>,
    Path(order_id): Path<Uuid>,
) -> AppResult<Json<OrderResponse>> {
    let response = OrderService::new(state).get_order(order_id).await?;
    Ok(Json(response))
}
