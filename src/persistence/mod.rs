use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool, postgres::PgPoolOptions, query, query_as};
use uuid::Uuid;

use crate::{
    domain::entities::{
        LineItem, Order, PaymentFinality, PaymentRail, Quote, Receipt, SettlementPreference,
    },
    errors::{ApiError, AppResult},
};

#[async_trait]
pub trait QuoteRepository: Send + Sync {
    async fn insert_quote(&self, quote: &Quote) -> AppResult<()>;
    async fn get_quote(&self, quote_id: Uuid) -> AppResult<Option<Quote>>;
}

#[async_trait]
pub trait OrderRepository: Send + Sync {
    async fn insert_order(&self, order: &Order) -> AppResult<()>;
    async fn update_order(&self, order: &Order) -> AppResult<()>;
    async fn get_order(&self, order_id: Uuid) -> AppResult<Option<Order>>;
    async fn get_order_by_quote_id(&self, quote_id: Uuid) -> AppResult<Option<Order>>;
}

#[async_trait]
pub trait ReceiptRepository: Send + Sync {
    async fn insert_receipt(&self, receipt: &Receipt) -> AppResult<()>;
    async fn get_receipt_by_order_id(&self, order_id: Uuid) -> AppResult<Option<Receipt>>;
}

#[async_trait]
pub trait NonceRepository: Send + Sync {
    async fn insert_nonce(
        &self,
        public_key: String,
        nonce: String,
        message_id: Uuid,
        expires_at: DateTime<Utc>,
    ) -> AppResult<bool>;
}

pub type DynQuoteRepository = Arc<dyn QuoteRepository>;
pub type DynOrderRepository = Arc<dyn OrderRepository>;
pub type DynReceiptRepository = Arc<dyn ReceiptRepository>;
pub type DynNonceRepository = Arc<dyn NonceRepository>;

pub async fn connect(database_url: &str) -> AppResult<PgPool> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|error| ApiError::internal(format!("failed to connect to postgres: {error}")))
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryQuoteRepository {
    inner: Arc<RwLock<HashMap<Uuid, Quote>>>,
}

#[async_trait]
impl QuoteRepository for InMemoryQuoteRepository {
    async fn insert_quote(&self, quote: &Quote) -> AppResult<()> {
        self.inner
            .write()
            .map_err(|_| ApiError::internal("quote repository lock poisoned"))?
            .insert(quote.id, quote.clone());
        Ok(())
    }

    async fn get_quote(&self, quote_id: Uuid) -> AppResult<Option<Quote>> {
        Ok(self
            .inner
            .read()
            .map_err(|_| ApiError::internal("quote repository lock poisoned"))?
            .get(&quote_id)
            .cloned())
    }
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryOrderRepository {
    inner: Arc<RwLock<HashMap<Uuid, Order>>>,
}

#[async_trait]
impl OrderRepository for InMemoryOrderRepository {
    async fn insert_order(&self, order: &Order) -> AppResult<()> {
        self.inner
            .write()
            .map_err(|_| ApiError::internal("order repository lock poisoned"))?
            .insert(order.id, order.clone());
        Ok(())
    }

    async fn update_order(&self, order: &Order) -> AppResult<()> {
        self.inner
            .write()
            .map_err(|_| ApiError::internal("order repository lock poisoned"))?
            .insert(order.id, order.clone());
        Ok(())
    }

    async fn get_order(&self, order_id: Uuid) -> AppResult<Option<Order>> {
        Ok(self
            .inner
            .read()
            .map_err(|_| ApiError::internal("order repository lock poisoned"))?
            .get(&order_id)
            .cloned())
    }

    async fn get_order_by_quote_id(&self, quote_id: Uuid) -> AppResult<Option<Order>> {
        Ok(self
            .inner
            .read()
            .map_err(|_| ApiError::internal("order repository lock poisoned"))?
            .values()
            .find(|order| order.quote_id == quote_id)
            .cloned())
    }
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryReceiptRepository {
    inner: Arc<RwLock<HashMap<Uuid, Receipt>>>,
}

#[async_trait]
impl ReceiptRepository for InMemoryReceiptRepository {
    async fn insert_receipt(&self, receipt: &Receipt) -> AppResult<()> {
        self.inner
            .write()
            .map_err(|_| ApiError::internal("receipt repository lock poisoned"))?
            .insert(receipt.order_id, receipt.clone());
        Ok(())
    }

    async fn get_receipt_by_order_id(&self, order_id: Uuid) -> AppResult<Option<Receipt>> {
        Ok(self
            .inner
            .read()
            .map_err(|_| ApiError::internal("receipt repository lock poisoned"))?
            .get(&order_id)
            .cloned())
    }
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryNonceRepository {
    inner: Arc<RwLock<HashSet<(String, String)>>>,
}

#[async_trait]
impl NonceRepository for InMemoryNonceRepository {
    async fn insert_nonce(
        &self,
        public_key: String,
        nonce: String,
        _message_id: Uuid,
        _expires_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| ApiError::internal("nonce repository lock poisoned"))?;
        Ok(guard.insert((public_key, nonce)))
    }
}

#[derive(Debug, Clone)]
pub struct PostgresQuoteRepository {
    pool: PgPool,
}

impl PostgresQuoteRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Debug, Clone)]
pub struct PostgresOrderRepository {
    pool: PgPool,
}

impl PostgresOrderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Debug, Clone)]
pub struct PostgresReceiptRepository {
    pool: PgPool,
}

impl PostgresReceiptRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Debug, Clone)]
pub struct PostgresNonceRepository {
    pool: PgPool,
}

impl PostgresNonceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QuotePayloadSnapshot {
    items: Vec<LineItem>,
    settlement_preference: SettlementPreference,
    callback_relays: Vec<String>,
    buyer_reference: Option<String>,
    accepted_rails: Vec<PaymentRail>,
}

#[derive(Debug, FromRow)]
struct QuoteRow {
    id: Uuid,
    order_id: Uuid,
    buyer_pubkey: String,
    seller_pubkey: String,
    quote_payload: Value,
    total_sats: i64,
    status: String,
    expires_at: DateTime<Utc>,
    quote_lock_until: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct OrderRow {
    id: Uuid,
    quote_id: Uuid,
    buyer_pubkey: String,
    seller_pubkey: String,
    state: String,
    selected_rail: Option<String>,
    checkout_idempotency_key: Option<String>,
    payment_confirm_idempotency_key: Option<String>,
    lightning_invoice: Option<String>,
    lightning_payment_hash: Option<String>,
    onchain_address: Option<String>,
    payment_amount_sats: Option<i64>,
    settlement_proof: Option<Value>,
    onchain_confirmations: Option<i32>,
    last_error_code: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct ReceiptRow {
    id: Uuid,
    order_id: Uuid,
    rail: String,
    receipt_hash: String,
    nostr_event_id: Option<String>,
    receipt_payload: Value,
    created_at: DateTime<Utc>,
}

impl TryFrom<QuoteRow> for Quote {
    type Error = ApiError;

    fn try_from(row: QuoteRow) -> Result<Self, Self::Error> {
        let snapshot: QuotePayloadSnapshot = serde_json::from_value(row.quote_payload)
            .map_err(|error| ApiError::internal(format!("invalid quote snapshot: {error}")))?;
        Ok(Self {
            id: row.id,
            order_id: row.order_id,
            buyer_pubkey: row.buyer_pubkey,
            seller_pubkey: row.seller_pubkey,
            items: snapshot.items,
            settlement_preference: snapshot.settlement_preference,
            callback_relays: snapshot.callback_relays,
            buyer_reference: snapshot.buyer_reference,
            total_sats: row.total_sats,
            status: row.status.parse()?,
            expires_at: row.expires_at,
            quote_lock_until: row.quote_lock_until,
            accepted_rails: snapshot.accepted_rails,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

impl TryFrom<OrderRow> for Order {
    type Error = ApiError;

    fn try_from(row: OrderRow) -> Result<Self, Self::Error> {
        let selected_rail = row.selected_rail.map(|value| value.parse()).transpose()?;
        let settlement_proof = row
            .settlement_proof
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| ApiError::internal(format!("invalid settlement proof: {error}")))?;
        Ok(Self {
            id: row.id,
            quote_id: row.quote_id,
            buyer_pubkey: row.buyer_pubkey,
            seller_pubkey: row.seller_pubkey,
            state: row.state.parse()?,
            selected_rail,
            checkout_idempotency_key: row.checkout_idempotency_key,
            payment_confirm_idempotency_key: row.payment_confirm_idempotency_key,
            lightning_invoice: row.lightning_invoice,
            lightning_payment_hash: row.lightning_payment_hash,
            onchain_address: row.onchain_address,
            payment_amount_sats: row.payment_amount_sats,
            settlement_proof,
            onchain_confirmations: row.onchain_confirmations.map(|value| value as u32),
            last_error_code: row.last_error_code,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

impl TryFrom<ReceiptRow> for Receipt {
    type Error = ApiError;

    fn try_from(row: ReceiptRow) -> Result<Self, Self::Error> {
        let finality: PaymentFinality = row
            .receipt_payload
            .get("finality")
            .and_then(Value::as_str)
            .map(|value| match value {
                "settled" => Ok(PaymentFinality::Settled),
                "confirmed" => Ok(PaymentFinality::Confirmed),
                "pending" => Ok(PaymentFinality::Pending),
                _ => Err(ApiError::internal(format!(
                    "unknown payment finality: {value}"
                ))),
            })
            .transpose()?
            .unwrap_or(PaymentFinality::Settled);

        Ok(Self {
            id: row.id,
            order_id: row.order_id,
            rail: row.rail.parse()?,
            receipt_hash: row.receipt_hash,
            nostr_event_id: row.nostr_event_id,
            finality,
            payload: row.receipt_payload,
            created_at: row.created_at,
        })
    }
}

#[async_trait]
impl QuoteRepository for PostgresQuoteRepository {
    async fn insert_quote(&self, quote: &Quote) -> AppResult<()> {
        let snapshot = QuotePayloadSnapshot {
            items: quote.items.clone(),
            settlement_preference: quote.settlement_preference,
            callback_relays: quote.callback_relays.clone(),
            buyer_reference: quote.buyer_reference.clone(),
            accepted_rails: quote.accepted_rails.clone(),
        };
        let snapshot = serde_json::to_value(snapshot)
            .map_err(|error| ApiError::internal(format!("failed to serialize quote: {error}")))?;
        query(
            r#"
            INSERT INTO quotes
                (id, order_id, buyer_pubkey, seller_pubkey, quote_payload, total_sats, status, expires_at, quote_lock_until, created_at, updated_at)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(quote.id)
        .bind(quote.order_id)
        .bind(&quote.buyer_pubkey)
        .bind(&quote.seller_pubkey)
        .bind(snapshot)
        .bind(quote.total_sats)
        .bind(quote.status.to_string())
        .bind(quote.expires_at)
        .bind(quote.quote_lock_until)
        .bind(quote.created_at)
        .bind(quote.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|error| ApiError::internal(format!("failed to insert quote: {error}")))?;
        Ok(())
    }

    async fn get_quote(&self, quote_id: Uuid) -> AppResult<Option<Quote>> {
        let row = query_as::<_, QuoteRow>("SELECT * FROM quotes WHERE id = $1")
            .bind(quote_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| ApiError::internal(format!("failed to fetch quote: {error}")))?;
        row.map(TryInto::try_into).transpose()
    }
}

#[async_trait]
impl OrderRepository for PostgresOrderRepository {
    async fn insert_order(&self, order: &Order) -> AppResult<()> {
        query(
            r#"
            INSERT INTO orders
                (id, quote_id, buyer_pubkey, seller_pubkey, state, selected_rail, checkout_idempotency_key, payment_confirm_idempotency_key, lightning_invoice, lightning_payment_hash, onchain_address, payment_amount_sats, settlement_proof, onchain_confirmations, last_error_code, created_at, updated_at)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            "#,
        )
        .bind(order.id)
        .bind(order.quote_id)
        .bind(&order.buyer_pubkey)
        .bind(&order.seller_pubkey)
        .bind(order.state.to_string())
        .bind(order.selected_rail.map(|value| value.to_string()))
        .bind(&order.checkout_idempotency_key)
        .bind(&order.payment_confirm_idempotency_key)
        .bind(&order.lightning_invoice)
        .bind(&order.lightning_payment_hash)
        .bind(&order.onchain_address)
        .bind(order.payment_amount_sats)
        .bind(order
            .settlement_proof
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| ApiError::internal(format!("failed to serialize settlement proof: {error}")))?)
        .bind(order.onchain_confirmations.map(|value| value as i32))
        .bind(&order.last_error_code)
        .bind(order.created_at)
        .bind(order.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|error| ApiError::internal(format!("failed to insert order: {error}")))?;
        Ok(())
    }

    async fn update_order(&self, order: &Order) -> AppResult<()> {
        query(
            r#"
            UPDATE orders
            SET state = $2,
                selected_rail = $3,
                checkout_idempotency_key = $4,
                payment_confirm_idempotency_key = $5,
                lightning_invoice = $6,
                lightning_payment_hash = $7,
                onchain_address = $8,
                payment_amount_sats = $9,
                settlement_proof = $10,
                onchain_confirmations = $11,
                last_error_code = $12,
                updated_at = $13
            WHERE id = $1
            "#,
        )
        .bind(order.id)
        .bind(order.state.to_string())
        .bind(order.selected_rail.map(|value| value.to_string()))
        .bind(&order.checkout_idempotency_key)
        .bind(&order.payment_confirm_idempotency_key)
        .bind(&order.lightning_invoice)
        .bind(&order.lightning_payment_hash)
        .bind(&order.onchain_address)
        .bind(order.payment_amount_sats)
        .bind(
            order
                .settlement_proof
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|error| {
                    ApiError::internal(format!("failed to serialize settlement proof: {error}"))
                })?,
        )
        .bind(order.onchain_confirmations.map(|value| value as i32))
        .bind(&order.last_error_code)
        .bind(order.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|error| ApiError::internal(format!("failed to update order: {error}")))?;
        Ok(())
    }

    async fn get_order(&self, order_id: Uuid) -> AppResult<Option<Order>> {
        let row = query_as::<_, OrderRow>("SELECT * FROM orders WHERE id = $1")
            .bind(order_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| ApiError::internal(format!("failed to fetch order: {error}")))?;
        row.map(TryInto::try_into).transpose()
    }

    async fn get_order_by_quote_id(&self, quote_id: Uuid) -> AppResult<Option<Order>> {
        let row = query_as::<_, OrderRow>("SELECT * FROM orders WHERE quote_id = $1")
            .bind(quote_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| {
                ApiError::internal(format!("failed to fetch order by quote: {error}"))
            })?;
        row.map(TryInto::try_into).transpose()
    }
}

#[async_trait]
impl ReceiptRepository for PostgresReceiptRepository {
    async fn insert_receipt(&self, receipt: &Receipt) -> AppResult<()> {
        query(
            r#"
            INSERT INTO receipts
                (id, order_id, rail, receipt_hash, nostr_event_id, receipt_payload, created_at)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(receipt.id)
        .bind(receipt.order_id)
        .bind(receipt.rail.to_string())
        .bind(&receipt.receipt_hash)
        .bind(&receipt.nostr_event_id)
        .bind(&receipt.payload)
        .bind(receipt.created_at)
        .execute(&self.pool)
        .await
        .map_err(|error| ApiError::internal(format!("failed to insert receipt: {error}")))?;
        Ok(())
    }

    async fn get_receipt_by_order_id(&self, order_id: Uuid) -> AppResult<Option<Receipt>> {
        let row = query_as::<_, ReceiptRow>(
            "SELECT * FROM receipts WHERE order_id = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(order_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| ApiError::internal(format!("failed to fetch receipt: {error}")))?;
        row.map(TryInto::try_into).transpose()
    }
}

#[async_trait]
impl NonceRepository for PostgresNonceRepository {
    async fn insert_nonce(
        &self,
        public_key: String,
        nonce: String,
        message_id: Uuid,
        expires_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let result = query(
            r#"
            INSERT INTO nonces (public_key, nonce, message_id, expires_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(public_key)
        .bind(nonce)
        .bind(message_id)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|error| ApiError::internal(format!("failed to insert nonce: {error}")))?;
        Ok(result.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::{InMemoryNonceRepository, NonceRepository};
    use crate::errors::AppResult;

    #[tokio::test]
    async fn in_memory_nonce_repository_rejects_replay() -> AppResult<()> {
        let repository = InMemoryNonceRepository::default();
        let first = repository
            .insert_nonce(
                "buyer-pubkey".into(),
                "nonce-1".into(),
                Uuid::new_v4(),
                Utc::now(),
            )
            .await?;
        let second = repository
            .insert_nonce(
                "buyer-pubkey".into(),
                "nonce-1".into(),
                Uuid::new_v4(),
                Utc::now(),
            )
            .await?;

        assert!(first);
        assert!(!second);
        Ok(())
    }
}
