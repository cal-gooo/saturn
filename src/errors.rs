use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

pub type AppResult<T> = Result<T, ApiError>;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    BadRequest,
    SchemaInvalid,
    SignatureInvalid,
    TimestampOutOfWindow,
    ReplayNonceReused,
    IdempotencyMissing,
    IdempotencyConflict,
    QuoteExpired,
    StateTransitionInvalid,
    PaymentVerificationFailed,
    PaymentFinalityPending,
    ResourceNotFound,
    InternalError,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ApiError {
    pub status: StatusCode,
    pub code: ApiErrorCode,
    pub message: String,
    pub details: Option<Value>,
}

impl ApiError {
    pub fn new(
        status: StatusCode,
        code: ApiErrorCode,
        message: impl Into<String>,
        details: Option<Value>,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details,
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::BadRequest,
            message,
            None,
        )
    }

    pub fn schema_invalid(details: Value) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::SchemaInvalid,
            "request body failed validation",
            Some(details),
        )
    }

    pub fn signature_invalid(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            ApiErrorCode::SignatureInvalid,
            message,
            None,
        )
    }

    pub fn timestamp_out_of_window(max_skew_seconds: u64) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            ApiErrorCode::TimestampOutOfWindow,
            format!("timestamp is outside the ±{max_skew_seconds}s window"),
            None,
        )
    }

    pub fn replay_nonce_reused() -> Self {
        Self::new(
            StatusCode::CONFLICT,
            ApiErrorCode::ReplayNonceReused,
            "nonce has already been used",
            None,
        )
    }

    pub fn idempotency_missing() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::IdempotencyMissing,
            "Idempotency-Key header is required",
            None,
        )
    }

    pub fn idempotency_conflict() -> Self {
        Self::new(
            StatusCode::CONFLICT,
            ApiErrorCode::IdempotencyConflict,
            "idempotency key conflicts with existing request state",
            None,
        )
    }

    pub fn quote_expired() -> Self {
        Self::new(
            StatusCode::GONE,
            ApiErrorCode::QuoteExpired,
            "quote is expired or outside the lock window",
            None,
        )
    }

    pub fn state_transition_invalid(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            ApiErrorCode::StateTransitionInvalid,
            message,
            None,
        )
    }

    pub fn payment_verification_failed(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ApiErrorCode::PaymentVerificationFailed,
            message,
            None,
        )
    }

    pub fn payment_finality_pending(confirmations: u32, required: u32) -> Self {
        Self::new(
            StatusCode::ACCEPTED,
            ApiErrorCode::PaymentFinalityPending,
            format!("on-chain payment has {confirmations} confirmations, requires {required}"),
            None,
        )
    }

    pub fn resource_not_found(resource: impl Into<String>) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            ApiErrorCode::ResourceNotFound,
            format!("{} not found", resource.into()),
            None,
        )
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::InternalError,
            message,
            None,
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "code": self.code,
                "message": self.message,
                "details": self.details,
            }
        });

        (self.status, Json(body)).into_response()
    }
}
