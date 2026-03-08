pub mod config;

use std::sync::Arc;

use axum::{
    Router,
    http::HeaderName,
    middleware,
    routing::{get, post},
};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

use crate::{
    api::handlers,
    nostr::{DynNostrPublisher, MockNostrPublisher},
    payments::{DynLightningAdapter, DynOnChainAdapter, MockLightningAdapter, MockOnChainAdapter},
    persistence::{
        DynNonceRepository, DynOrderRepository, DynQuoteRepository, DynReceiptRepository,
        InMemoryNonceRepository, InMemoryOrderRepository, InMemoryQuoteRepository,
        InMemoryReceiptRepository,
    },
    privacy::{DisabledCoinjoinClient, DynCoinjoinClient},
};

pub use config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub quote_repository: DynQuoteRepository,
    pub order_repository: DynOrderRepository,
    pub receipt_repository: DynReceiptRepository,
    pub nonce_repository: DynNonceRepository,
    pub lightning_adapter: DynLightningAdapter,
    pub onchain_adapter: DynOnChainAdapter,
    pub nostr_publisher: DynNostrPublisher,
    pub coinjoin_client: DynCoinjoinClient,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: AppConfig,
        quote_repository: DynQuoteRepository,
        order_repository: DynOrderRepository,
        receipt_repository: DynReceiptRepository,
        nonce_repository: DynNonceRepository,
        lightning_adapter: DynLightningAdapter,
        onchain_adapter: DynOnChainAdapter,
        nostr_publisher: DynNostrPublisher,
        coinjoin_client: DynCoinjoinClient,
    ) -> Self {
        Self {
            config,
            quote_repository,
            order_repository,
            receipt_repository,
            nonce_repository,
            lightning_adapter,
            onchain_adapter,
            nostr_publisher,
            coinjoin_client,
        }
    }

    pub fn for_tests() -> Self {
        let config = AppConfig::for_tests();
        Self {
            quote_repository: Arc::new(InMemoryQuoteRepository::default()),
            order_repository: Arc::new(InMemoryOrderRepository::default()),
            receipt_repository: Arc::new(InMemoryReceiptRepository::default()),
            nonce_repository: Arc::new(InMemoryNonceRepository::default()),
            lightning_adapter: Arc::new(MockLightningAdapter),
            onchain_adapter: Arc::new(MockOnChainAdapter),
            nostr_publisher: Arc::new(MockNostrPublisher::new(config.nostr_relays.clone())),
            coinjoin_client: Arc::new(DisabledCoinjoinClient),
            config,
        }
    }
}

pub fn build_router(state: AppState) -> Router {
    let correlation_header = HeaderName::from_static("x-correlation-id");

    Router::new()
        .route("/capabilities", get(handlers::get_capabilities))
        .route(
            "/quote",
            post(handlers::post_quote).route_layer(middleware::from_fn_with_state(
                state.clone(),
                crate::security::middleware::signed_request_middleware,
            )),
        )
        .route(
            "/checkout-intent",
            post(handlers::post_checkout_intent).route_layer(middleware::from_fn_with_state(
                state.clone(),
                crate::security::middleware::signed_request_middleware,
            )),
        )
        .route(
            "/payment/confirm",
            post(handlers::post_payment_confirm).route_layer(middleware::from_fn_with_state(
                state.clone(),
                crate::security::middleware::signed_request_middleware,
            )),
        )
        .route("/order/{id}", get(handlers::get_order))
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(correlation_header.clone()))
        .layer(SetRequestIdLayer::new(correlation_header, MakeRequestUuid))
        .with_state(state)
}
