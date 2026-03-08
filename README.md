# Saturn

<p align="center">
  <img src="assets/saturn-hero.svg" alt="Stylized Saturn planet banner for the Saturn repository" width="900">
</p>

Saturn is the Rust reference implementation for A2A Commerce Protocol: BTC-only settlement with Nostr-native identity, receipts, and relay redundancy.

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
├── apps
│   └── website
│       ├── docs
│       ├── public
│       ├── src
│       ├── index.html
│       ├── package.json
│       └── vite.config.js
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
├── package.json
├── src
│   ├── api
│   ├── app
│   ├── domain
│   ├── nostr
│   ├── payments
│   ├── persistence
│   ├── privacy
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
cargo run --bin saturn-server
```

4a. Run the official website from the monorepo:

```bash
npm install
npm run website:dev
```

The Svelte + Vite site lives in `apps/website`. The landing page is at `/` and the docs
landing page is at `/docs/`.

To enable the real LDK-backed adapters, set `APP__LIGHTNING_BACKEND=ldk` and/or
`APP__ONCHAIN_BACKEND=ldk`, then provide the shared LDK seed, storage path, and chain source
settings in `.env`.

Joinstr is available only as an optional sidecar for post-settlement on-chain privacy. To enable
it, set `APP__COINJOIN_BACKEND=joinstr_sidecar` and point `APP__JOINSTR_SIDECAR_URL` at a sidecar
endpoint that accepts `POST` requests with confirmed on-chain outputs. Saturn will enqueue those
outputs after a successful on-chain payment confirmation; it does not change the checkout flow or
block buyer settlement if the sidecar is unavailable.

Expected sidecar payload:

```json
{
  "order_id": "8b0f2643-e783-4f7a-81d4-52b3559b6d14",
  "merchant_nostr_pubkey": "4f355bdcb7cc0af728ef3cceb9615d90684bb5b2ca5f859ab0f0b704075871aa",
  "network": "testnet",
  "address": "tb1q...",
  "txid": "abc123...",
  "vout": 1,
  "amount_sats": 21000,
  "confirmations": 6,
  "receipt_event_id": "nostr-event-id",
  "queued_at": "2026-03-08T15:00:00Z"
}
```

5. Run tests:

```bash
cargo test
npm run website:build
```

For the live LDK regtest path, there are also ignored tests that boot bitcoind and electrs
locally and verify real settlement flows:

```bash
cargo test --test ldk_regtest -- --ignored --nocapture
cargo test payments::tests::ldk_lightning_adapter_round_trips_real_payment -- --ignored --nocapture
cargo test payments::tests::saturn_router_completes_real_lightning_checkout -- --ignored --nocapture
cargo test payments::tests::saturn_router_completes_real_onchain_checkout -- --ignored --nocapture
```

If `cargo` is not on your shell `PATH`, use:

```bash
export PATH="$(dirname "$(rustup which rustc)"):$PATH"
```

## Full Flow With curl

1. Export a signing secret:

```bash
export APP__MERCHANT_REQUEST_SIGNING_SECRET_KEY=2222222222222222222222222222222222222222222222222222222222222222
```

Saturn keeps its request-signing key separate from the Nostr relay identity key in `.env`.
For request signing, `sign-payload` will also read `APP__MERCHANT_REQUEST_SIGNING_SECRET_KEY`
directly from the environment.

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
