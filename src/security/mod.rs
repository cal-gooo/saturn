pub mod canonical;
pub mod middleware;
pub mod signing;

use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct VerifiedRequestContext {
    pub message_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub nonce: String,
    pub public_key: String,
    pub correlation_id: Option<String>,
}
