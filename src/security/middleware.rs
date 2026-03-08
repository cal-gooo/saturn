use axum::{
    body::{Body, to_bytes},
    extract::State,
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{Duration, Utc};
use serde_json::Value;

use crate::{
    app::AppState,
    errors::{ApiError, AppResult},
    security::{VerifiedRequestContext, signing::verify_signature},
};

pub async fn signed_request_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    match verify_request(state, request).await {
        Ok(request) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

async fn verify_request(state: AppState, request: Request<Body>) -> AppResult<Request<Body>> {
    let correlation_id = request
        .headers()
        .get("x-correlation-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, usize::MAX)
        .await
        .map_err(|error| ApiError::bad_request(format!("failed to read request body: {error}")))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        ApiError::bad_request(format!("request body must be valid JSON: {error}"))
    })?;
    let metadata = verify_signature(&value)?;
    let now = Utc::now();
    let drift = (now - metadata.timestamp).num_seconds().unsigned_abs();
    if drift > state.config.max_clock_skew_seconds {
        return Err(ApiError::timestamp_out_of_window(
            state.config.max_clock_skew_seconds,
        ));
    }

    let nonce_expires_at =
        metadata.timestamp + Duration::seconds(state.config.max_clock_skew_seconds as i64);
    let accepted = state
        .nonce_repository
        .insert_nonce(
            metadata.public_key.clone(),
            metadata.nonce.clone(),
            metadata.message_id,
            nonce_expires_at,
        )
        .await?;

    if !accepted {
        return Err(ApiError::replay_nonce_reused());
    }

    let mut request = Request::from_parts(parts, Body::from(bytes));
    request.extensions_mut().insert(VerifiedRequestContext {
        message_id: metadata.message_id,
        timestamp: metadata.timestamp,
        nonce: metadata.nonce,
        public_key: metadata.public_key,
        correlation_id,
    });
    Ok(request)
}
