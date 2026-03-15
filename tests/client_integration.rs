use chrono::Utc;
use tokio::net::TcpListener;

use saturn::{
    api::schemas::SettlementProofInput,
    app::{AppState, build_router},
    client::SaturnClient,
    domain::entities::{LineItem, PaymentRail, SettlementPreference},
};

const TEST_SECRET_KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";

async fn start_server() -> String {
    let state = AppState::for_tests();
    let router = build_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("should bind");
    let addr = listener.local_addr().expect("should have local addr");
    let base_url = format!("http://{addr}");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("server should run");
    });
    base_url
}

fn test_client(base_url: &str) -> SaturnClient {
    SaturnClient::builder(base_url, TEST_SECRET_KEY).build()
}

#[tokio::test]
async fn client_full_happy_path() {
    let base_url = start_server().await;
    let client = test_client(&base_url);

    let seller_pubkey = client.public_key_hex().expect("should derive pubkey");
    let buyer_pubkey =
        "02cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();

    // 1. Capabilities
    let caps = client
        .get_capabilities()
        .await
        .expect("capabilities should succeed");
    assert_eq!(caps.protocol, "a2ac");
    assert_eq!(caps.version, "0.1.0");

    // 2. Create quote
    let quote = client
        .create_quote(
            &buyer_pubkey,
            &seller_pubkey,
            vec![LineItem {
                sku: "agent-plan".into(),
                description: "Autonomous procurement plan".into(),
                quantity: 1,
                unit_price_sats: 21_000,
            }],
            SettlementPreference::LightningWithOnchainFallback,
            vec!["wss://relay.damus.io".into()],
            Some("client-sdk-test".into()),
        )
        .await
        .expect("quote should succeed");
    assert_eq!(quote.total_sats, 21_000);
    assert_eq!(quote.state, saturn::domain::entities::OrderState::Quoted);

    // 3. Checkout
    let checkout = client
        .create_checkout(
            quote.quote_id,
            PaymentRail::Lightning,
            "checkout-sdk-1",
            Some("buyer-ref-1".into()),
            Some(vec!["wss://nos.lol".into()]),
        )
        .await
        .expect("checkout should succeed");
    assert_eq!(
        checkout.state,
        saturn::domain::entities::OrderState::PaymentPending
    );
    assert!(!checkout.lightning_invoice.is_empty());
    assert!(!checkout.lightning_payment_hash.is_empty());

    // 4. Confirm payment
    let payment = client
        .confirm_payment(
            checkout.order_id,
            PaymentRail::Lightning,
            SettlementProofInput::Lightning {
                payment_hash: checkout.lightning_payment_hash.clone(),
                preimage: Some("mock-preimage".into()),
                settled_at: Utc::now(),
                amount_sats: 21_000,
            },
            "payment-sdk-1",
        )
        .await
        .expect("payment confirm should succeed");
    assert_eq!(payment.state, saturn::domain::entities::OrderState::Paid);

    // 5. Fulfill
    let fulfilled = client
        .fulfill_order(checkout.order_id)
        .await
        .expect("fulfill should succeed");
    assert_eq!(
        fulfilled.state,
        saturn::domain::entities::OrderState::Fulfilled
    );

    // 6. Get order
    let order = client
        .get_order(checkout.order_id)
        .await
        .expect("get order should succeed");
    assert_eq!(order.state, saturn::domain::entities::OrderState::Fulfilled);
    assert_eq!(order.quote_id, quote.quote_id);
}

#[tokio::test]
async fn client_receives_api_error_for_bad_state_transition() {
    let base_url = start_server().await;
    let client = test_client(&base_url);

    let seller_pubkey = client.public_key_hex().expect("should derive pubkey");
    let buyer_pubkey =
        "02cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();

    let quote = client
        .create_quote(
            &buyer_pubkey,
            &seller_pubkey,
            vec![LineItem {
                sku: "widget".into(),
                description: "A widget".into(),
                quantity: 1,
                unit_price_sats: 5_000,
            }],
            SettlementPreference::LightningOnly,
            vec!["wss://relay.damus.io".into()],
            None,
        )
        .await
        .expect("quote should succeed");

    // Try to fulfill a quoted (not paid) order — should fail
    let err = client
        .fulfill_order(quote.order_id)
        .await
        .expect_err("fulfill should fail on quoted order");

    match err {
        saturn::client::ClientError::Api { error, .. } => {
            assert_eq!(error.code, "state_transition_invalid");
        }
        other => panic!("expected Api error, got: {other:?}"),
    }
}
