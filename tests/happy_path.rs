mod common;

use axum::http::Method;
use chrono::Utc;
use serde_json::{Value, json};

use common::{app, json_request, signed_envelope, test_public_key};

#[tokio::test]
async fn full_happy_path_checkout_and_fulfillment_succeeds() {
    let app = app();
    let seller_pubkey = test_public_key();
    let buyer_pubkey =
        "02cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();

    let quote_request = signed_envelope(json!({
        "buyer_nostr_pubkey": buyer_pubkey,
        "seller_nostr_pubkey": seller_pubkey,
        "callback_relays": ["wss://relay.damus.io"],
        "items": [
            {
                "sku": "agent-plan",
                "description": "Autonomous procurement plan",
                "quantity": 1,
                "unit_price_sats": 21000
            }
        ],
        "settlement_preference": "lightning_with_onchain_fallback",
        "buyer_reference": "happy-path"
    }));

    let (quote_status, quote_body) =
        json_request(app.clone(), Method::POST, "/quote", quote_request, None)
            .await
            .expect("quote request should succeed");
    assert_eq!(quote_status, 200);

    let quote_id = quote_body["quote_id"].as_str().expect("quote id");
    let order_id = quote_body["order_id"].as_str().expect("order id");
    let lightning_checkout = signed_envelope(json!({
        "quote_id": quote_id,
        "selected_rail": "lightning",
        "buyer_reference": "buyer-order-1",
        "return_relays": ["wss://nos.lol"]
    }));

    let (checkout_status, checkout_body) = json_request(
        app.clone(),
        Method::POST,
        "/checkout-intent",
        lightning_checkout,
        Some("checkout-1"),
    )
    .await
    .expect("checkout request should succeed");
    assert_eq!(checkout_status, 200);
    assert_eq!(checkout_body["state"], "payment_pending");

    let payment_hash = checkout_body["lightning_payment_hash"]
        .as_str()
        .expect("payment hash");

    let payment_confirm = signed_envelope(json!({
        "order_id": order_id,
        "rail": "lightning",
        "settlement_proof": {
            "type": "lightning",
            "payment_hash": payment_hash,
            "preimage": "mock-preimage",
            "settled_at": Utc::now(),
            "amount_sats": 21000
        }
    }));

    let (payment_status, payment_body) = json_request(
        app.clone(),
        Method::POST,
        "/payment/confirm",
        payment_confirm,
        Some("payment-1"),
    )
    .await
    .expect("payment request should succeed");
    assert_eq!(payment_status, 200);
    assert_eq!(payment_body["state"], "paid");

    let fulfill_request = signed_envelope(json!({
        "order_id": order_id
    }));
    let (fulfill_status, fulfill_body) = json_request(
        app.clone(),
        Method::POST,
        &format!("/order/{order_id}/fulfill"),
        fulfill_request,
        None,
    )
    .await
    .expect("fulfill request should succeed");
    assert_eq!(fulfill_status, 200);
    assert_eq!(fulfill_body["state"], "fulfilled");

    let (order_status, order_body) = json_request(
        app,
        Method::GET,
        &format!("/order/{order_id}"),
        Value::Null,
        None,
    )
    .await
    .expect("order fetch should succeed");
    assert_eq!(order_status, 200);
    assert_eq!(order_body["state"], "fulfilled");
    assert_eq!(order_body["quote_id"], quote_id);
}

#[tokio::test]
async fn fulfill_rejects_invalid_state_transition() {
    let app = app();
    let seller_pubkey = test_public_key();
    let buyer_pubkey =
        "02cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();

    let quote_request = signed_envelope(json!({
        "buyer_nostr_pubkey": buyer_pubkey,
        "seller_nostr_pubkey": seller_pubkey,
        "callback_relays": ["wss://relay.damus.io"],
        "items": [
            {
                "sku": "agent-plan",
                "description": "Autonomous procurement plan",
                "quantity": 1,
                "unit_price_sats": 21000
            }
        ],
        "settlement_preference": "lightning_with_onchain_fallback",
        "buyer_reference": "invalid-fulfill-state"
    }));

    let (quote_status, quote_body) =
        json_request(app.clone(), Method::POST, "/quote", quote_request, None)
            .await
            .expect("quote request should succeed");
    assert_eq!(quote_status, 200);
    let order_id = quote_body["order_id"].as_str().expect("order id");

    let checkout_request = signed_envelope(json!({
        "quote_id": quote_body["quote_id"],
        "selected_rail": "lightning"
    }));
    let (checkout_status, checkout_body) = json_request(
        app.clone(),
        Method::POST,
        "/checkout-intent",
        checkout_request,
        Some("checkout-fulfill-invalid"),
    )
    .await
    .expect("checkout request should succeed");
    assert_eq!(checkout_status, 200);
    assert_eq!(checkout_body["state"], "payment_pending");

    let fulfill_request = signed_envelope(json!({
        "order_id": order_id
    }));
    let (fulfill_status, fulfill_body) = json_request(
        app,
        Method::POST,
        &format!("/order/{order_id}/fulfill"),
        fulfill_request,
        None,
    )
    .await
    .expect("fulfill request should return conflict");
    assert_eq!(fulfill_status, 409);
    assert_eq!(fulfill_body["error"]["code"], "state_transition_invalid");
}

#[tokio::test]
async fn fulfill_rejects_path_payload_id_mismatch() {
    let app = app();
    let seller_pubkey = test_public_key();
    let buyer_pubkey =
        "02cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();

    let quote_request = signed_envelope(json!({
        "buyer_nostr_pubkey": buyer_pubkey,
        "seller_nostr_pubkey": seller_pubkey,
        "callback_relays": ["wss://relay.damus.io"],
        "items": [
            {
                "sku": "agent-plan",
                "description": "Autonomous procurement plan",
                "quantity": 1,
                "unit_price_sats": 21000
            }
        ],
        "settlement_preference": "lightning_with_onchain_fallback"
    }));

    let (quote_status, quote_body) =
        json_request(app.clone(), Method::POST, "/quote", quote_request, None)
            .await
            .expect("quote request should succeed");
    assert_eq!(quote_status, 200);
    let order_id = quote_body["order_id"].as_str().expect("order id");

    let fulfill_request = signed_envelope(json!({
        "order_id": uuid::Uuid::new_v4()
    }));
    let (fulfill_status, fulfill_body) = json_request(
        app,
        Method::POST,
        &format!("/order/{order_id}/fulfill"),
        fulfill_request,
        None,
    )
    .await
    .expect("fulfill request should return bad request");
    assert_eq!(fulfill_status, 400);
    assert_eq!(fulfill_body["error"]["code"], "bad_request");
}
