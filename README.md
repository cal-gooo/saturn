# A2A Commerce Protocol

Original agent-to-agent commerce protocol and Rust reference implementation for BTC-only settlement with Nostr-native identity, receipts, and relay redundancy.

## Scope

- Buyer agent to seller agent commerce flow
- BTC-only settlement: Lightning first, optional on-chain fallback
- Nostr-native identity anchoring and receipt publication
- Rust stable, `axum`, `serde`, `sqlx`, `thiserror`, `tracing`
- Deterministic state machine and anti-replay request signing

## Repository Layout

```text
.
├── Cargo.toml
├── LICENSE
├── README.md
├── docker-compose.yml
├── docs
│   ├── nostr-events.md
│   ├── protocol-spec.md
│   └── examples
│       ├── capability-event.json
│       ├── payment-receipt-event.json
│       ├── quote-reference-event.json
│       └── status-update-event.json
├── migrations
│   └── 0001_init.sql
├── src
│   ├── api
│   ├── app
│   ├── domain
│   ├── nostr
│   ├── payments
│   ├── persistence
│   ├── security
│   ├── services
│   ├── errors.rs
│   ├── lib.rs
│   └── main.rs
└── tests
    ├── common
    └── happy_path.rs
```

## Phases

### Phase 1: Architecture + protocol

See [docs/protocol-spec.md](docs/protocol-spec.md) and [docs/nostr-events.md](docs/nostr-events.md).

Architecture notes live in [docs/architecture.md](docs/architecture.md).

### Phase 2: Code scaffold

The Rust scaffold lives under [src/lib.rs](src/lib.rs) and [src/main.rs](src/main.rs).

### Phase 3: Core implementation

Endpoints:

- `GET /capabilities`
- `POST /quote`
- `POST /checkout-intent`
- `POST /payment/confirm`
- `GET /order/:id`

### Phase 4: Run + test

1. Copy env:

```bash
cp .env.example .env
```

2. Start Postgres:

```bash
docker compose up -d postgres
```

3. Run migrations:

```bash
sqlx database create
sqlx migrate run
```

4. Start server:

```bash
cargo run --bin a2a-commerce-server
```

5. Run tests:

```bash
cargo test
```

If `cargo` is not on your shell `PATH`, use:

```bash
export PATH="$(dirname "$(rustup which rustc)"):$PATH"
```

## Full Flow With curl

1. Export a signing secret:

```bash
export A2AC_SIGNING_SECRET=1111111111111111111111111111111111111111111111111111111111111111
```

2. Create a quote request body:

```bash
cat > /tmp/quote.json <<'JSON'
{
  "message_id": "11111111-1111-4111-8111-111111111111",
  "timestamp": "2026-03-08T15:00:00Z",
  "nonce": "quote-nonce-1",
  "public_key": "",
  "signature": "",
  "buyer_nostr_pubkey": "02cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
  "seller_nostr_pubkey": "034f355bdcb7cc0af728ef3cceb9615d90684bb5b2ca5f859ab0f0b704075871aa",
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
  "buyer_reference": "demo-order-1"
}
JSON
```

3. Sign and submit the quote:

```bash
cargo run --bin sign-payload < /tmp/quote.json > /tmp/quote.signed.json
curl -s http://127.0.0.1:3000/quote \
  -H 'content-type: application/json' \
  --data @/tmp/quote.signed.json | tee /tmp/quote.response.json
```

4. Start checkout:

```bash
QUOTE_ID="$(jq -r '.quote_id' /tmp/quote.response.json)"
cat > /tmp/checkout.json <<JSON
{
  "message_id": "22222222-2222-4222-8222-222222222222",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "nonce": "checkout-nonce-1",
  "public_key": "",
  "signature": "",
  "quote_id": "$QUOTE_ID",
  "selected_rail": "lightning",
  "buyer_reference": "demo-checkout-1",
  "return_relays": ["wss://nos.lol"]
}
JSON
cargo run --bin sign-payload < /tmp/checkout.json > /tmp/checkout.signed.json
curl -s http://127.0.0.1:3000/checkout-intent \
  -H 'content-type: application/json' \
  -H 'Idempotency-Key: checkout-1' \
  --data @/tmp/checkout.signed.json | tee /tmp/checkout.response.json
```

5. Confirm Lightning payment:

```bash
ORDER_ID="$(jq -r '.order_id' /tmp/checkout.response.json)"
PAYMENT_HASH="$(jq -r '.lightning_payment_hash' /tmp/checkout.response.json)"
cat > /tmp/payment-confirm.json <<JSON
{
  "message_id": "33333333-3333-4333-8333-333333333333",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "nonce": "payment-nonce-1",
  "public_key": "",
  "signature": "",
  "order_id": "$ORDER_ID",
  "rail": "lightning",
  "settlement_proof": {
    "type": "lightning",
    "payment_hash": "$PAYMENT_HASH",
    "preimage": "mock-preimage",
    "settled_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "amount_sats": 21000
  }
}
JSON
cargo run --bin sign-payload < /tmp/payment-confirm.json > /tmp/payment-confirm.signed.json
curl -s http://127.0.0.1:3000/payment/confirm \
  -H 'content-type: application/json' \
  -H 'Idempotency-Key: payment-1' \
  --data @/tmp/payment-confirm.signed.json | tee /tmp/payment.response.json
```

6. Fetch the order:

```bash
curl -s "http://127.0.0.1:3000/order/$ORDER_ID" | jq
```

## Open source

This repository is published as an open source project under the MIT license in [LICENSE](LICENSE).

Community and governance files:

- [CONTRIBUTING.md](CONTRIBUTING.md)
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- [SECURITY.md](SECURITY.md)
- [SUPPORT.md](SUPPORT.md)
