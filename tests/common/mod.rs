use axum::{
    Router,
    body::Body,
    http::{Method, Request},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::util::ServiceExt;
use uuid::Uuid;

use a2a_commerce_protocol::{
    app::{AppState, build_router},
    errors::AppResult,
    security::signing::{derive_public_key, sign_value},
};

pub fn test_secret_key() -> &'static str {
    "1111111111111111111111111111111111111111111111111111111111111111"
}

pub fn test_public_key() -> String {
    derive_public_key(test_secret_key()).expect("public key derivation should work")
}

pub fn app() -> Router {
    build_router(AppState::for_tests())
}

pub fn signed_payload(mut payload: Value) -> Value {
    payload["public_key"] = Value::String(test_public_key());
    payload["signature"] = Value::String(String::new());
    sign_value(&mut payload, test_secret_key()).expect("payload signing should work");
    payload
}

pub async fn json_request(
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
    let request = builder
        .body(Body::from(body.to_string()))
        .expect("request should build");
    let response = app.oneshot(request).await.expect("router should respond");
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

pub fn signed_envelope(payload: Value) -> Value {
    let mut object = payload
        .as_object()
        .cloned()
        .expect("payload must be an object");
    object.insert("message_id".into(), serde_json::json!(Uuid::new_v4()));
    object.insert("timestamp".into(), serde_json::json!(chrono::Utc::now()));
    object.insert(
        "nonce".into(),
        serde_json::json!(format!("nonce-{}", Uuid::new_v4())),
    );
    object.insert("public_key".into(), serde_json::json!(test_public_key()));
    object.insert("signature".into(), serde_json::json!(""));
    signed_payload(Value::Object(object))
}
