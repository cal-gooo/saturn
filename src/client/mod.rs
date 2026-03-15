use chrono::Utc;
use reqwest::{Client as HttpClient, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    api::schemas::{
        CapabilitiesResponse, CheckoutIntentResponse, OrderResponse, PaymentConfirmResponse,
        QuoteResponse, SettlementProofInput,
    },
    domain::entities::{LineItem, PaymentRail, SettlementPreference},
    security::signing::{derive_public_key, sign_value},
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClientApiError {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error: {status} — {body}")]
    Http { status: StatusCode, body: String },

    #[error("API error ({status}): {error:?}")]
    Api {
        status: StatusCode,
        error: ClientApiError,
    },

    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("signing failed: {0}")]
    Signing(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type ClientResult<T> = Result<T, ClientError>;

pub struct SaturnClient {
    http: HttpClient,
    base_url: String,
    secret_key_hex: String,
}

pub struct SaturnClientBuilder {
    base_url: String,
    secret_key_hex: String,
    http: Option<HttpClient>,
}

impl SaturnClientBuilder {
    pub fn new(base_url: impl Into<String>, secret_key_hex: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            secret_key_hex: secret_key_hex.into(),
            http: None,
        }
    }

    pub fn http_client(mut self, client: HttpClient) -> Self {
        self.http = Some(client);
        self
    }

    pub fn build(self) -> SaturnClient {
        let base_url = self.base_url.trim_end_matches('/').to_owned();
        SaturnClient {
            http: self.http.unwrap_or_default(),
            base_url,
            secret_key_hex: self.secret_key_hex,
        }
    }
}

impl SaturnClient {
    pub fn builder(
        base_url: impl Into<String>,
        secret_key_hex: impl Into<String>,
    ) -> SaturnClientBuilder {
        SaturnClientBuilder::new(base_url, secret_key_hex)
    }

    pub fn public_key_hex(&self) -> ClientResult<String> {
        derive_public_key(&self.secret_key_hex).map_err(|e| ClientError::Signing(e.to_string()))
    }

    // --- Unsigned endpoints ---

    pub async fn get_capabilities(&self) -> ClientResult<CapabilitiesResponse> {
        let url = format!("{}/capabilities", self.base_url);
        let response = self.http.get(&url).send().await?;
        parse_response(response).await
    }

    pub async fn get_order(&self, order_id: Uuid) -> ClientResult<OrderResponse> {
        let url = format!("{}/order/{}", self.base_url, order_id);
        let response = self.http.get(&url).send().await?;
        parse_response(response).await
    }

    // --- Signed endpoints ---

    pub async fn create_quote(
        &self,
        buyer_nostr_pubkey: &str,
        seller_nostr_pubkey: &str,
        items: Vec<LineItem>,
        settlement_preference: SettlementPreference,
        callback_relays: Vec<String>,
        buyer_reference: Option<String>,
    ) -> ClientResult<QuoteResponse> {
        let payload = json!({
            "message_id": Uuid::new_v4().to_string(),
            "timestamp": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "nonce": Uuid::new_v4().to_string(),
            "public_key": "",
            "signature": "",
            "buyer_nostr_pubkey": buyer_nostr_pubkey,
            "seller_nostr_pubkey": seller_nostr_pubkey,
            "items": items,
            "settlement_preference": settlement_preference,
            "callback_relays": callback_relays,
            "buyer_reference": buyer_reference,
        });

        let url = format!("{}/quote", self.base_url);
        self.signed_post(&url, payload, None).await
    }

    pub async fn create_checkout(
        &self,
        quote_id: Uuid,
        selected_rail: PaymentRail,
        idempotency_key: &str,
        buyer_reference: Option<String>,
        return_relays: Option<Vec<String>>,
    ) -> ClientResult<CheckoutIntentResponse> {
        let payload = json!({
            "message_id": Uuid::new_v4().to_string(),
            "timestamp": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "nonce": Uuid::new_v4().to_string(),
            "public_key": "",
            "signature": "",
            "quote_id": quote_id,
            "selected_rail": selected_rail,
            "buyer_reference": buyer_reference,
            "return_relays": return_relays,
        });

        let url = format!("{}/checkout-intent", self.base_url);
        self.signed_post(&url, payload, Some(idempotency_key)).await
    }

    pub async fn confirm_payment(
        &self,
        order_id: Uuid,
        rail: PaymentRail,
        settlement_proof: SettlementProofInput,
        idempotency_key: &str,
    ) -> ClientResult<PaymentConfirmResponse> {
        let payload = json!({
            "message_id": Uuid::new_v4().to_string(),
            "timestamp": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "nonce": Uuid::new_v4().to_string(),
            "public_key": "",
            "signature": "",
            "order_id": order_id,
            "rail": rail,
            "settlement_proof": settlement_proof,
        });

        let url = format!("{}/payment/confirm", self.base_url);
        self.signed_post(&url, payload, Some(idempotency_key)).await
    }

    pub async fn fulfill_order(&self, order_id: Uuid) -> ClientResult<OrderResponse> {
        let payload = json!({
            "message_id": Uuid::new_v4().to_string(),
            "timestamp": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "nonce": Uuid::new_v4().to_string(),
            "public_key": "",
            "signature": "",
            "order_id": order_id,
        });

        let url = format!("{}/order/{}/fulfill", self.base_url, order_id);
        self.signed_post(&url, payload, None).await
    }

    // --- Internal helpers ---

    async fn signed_post<T: DeserializeOwned>(
        &self,
        url: &str,
        mut payload: Value,
        idempotency_key: Option<&str>,
    ) -> ClientResult<T> {
        sign_value(&mut payload, &self.secret_key_hex)
            .map_err(|e| ClientError::Signing(e.to_string()))?;

        let mut request = self
            .http
            .post(url)
            .header("content-type", "application/json")
            .json(&payload);

        if let Some(key) = idempotency_key {
            request = request.header("Idempotency-Key", key);
        }

        let response = request.send().await?;
        parse_response(response).await
    }
}

async fn parse_response<T: DeserializeOwned>(response: reqwest::Response) -> ClientResult<T> {
    let status = response.status();
    if status.is_success() {
        let body = response.text().await?;
        serde_json::from_str(&body).map_err(ClientError::Json)
    } else {
        let body = response.text().await?;
        if let Ok(wrapper) = serde_json::from_str::<Value>(&body)
            && let Some(error_obj) = wrapper.get("error")
            && let Ok(api_error) = serde_json::from_value::<ClientApiError>(error_obj.clone())
        {
            return Err(ClientError::Api {
                status,
                error: api_error,
            });
        }
        Err(ClientError::Http { status, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_creates_client_and_derives_public_key() {
        let client = SaturnClient::builder(
            "http://127.0.0.1:3000",
            "1111111111111111111111111111111111111111111111111111111111111111",
        )
        .build();

        let pubkey = client.public_key_hex().expect("should derive public key");
        assert!(!pubkey.is_empty());
        assert!(
            pubkey.len() == 66,
            "compressed public key should be 33 bytes / 66 hex chars"
        );
    }

    #[test]
    fn builder_trims_trailing_slash() {
        let client = SaturnClient::builder(
            "http://127.0.0.1:3000/",
            "1111111111111111111111111111111111111111111111111111111111111111",
        )
        .build();

        assert_eq!(client.base_url, "http://127.0.0.1:3000");
    }
}
