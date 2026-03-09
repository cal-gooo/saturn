# Saturn Docs

Saturn is a seller-side Bitcoin commerce server. It exposes a signed HTTP API for quote, checkout, and payment confirmation, uses LDK-backed rails for settlement, and publishes capabilities and receipts to Nostr.

This documentation is the fast orientation layer for the repo. It covers the protocol shape, the implementation architecture, and the Nostr event model behind Saturn.

## What Saturn does

- Accepts signed buyer-agent requests over HTTP
- Prices orders in sats and creates deterministic quotes
- Issues Lightning invoices with optional on-chain fallback
- Verifies settlement and stores receipts
- Publishes capabilities, quote references, and receipts to Nostr
- Optionally hands confirmed on-chain outputs to a Joinstr sidecar for treasury privacy

## Quick start

1. Copy the environment file.
2. Start Postgres.
3. Run the Rust server.
4. Open the website workspace if you want the landing page locally.

```bash
cp .env.example .env
docker compose up -d postgres
cargo run --bin saturn-server
npm install
npm run website:dev
```

If `cargo` is not on your shell `PATH`, use:

```bash
export PATH="$(dirname "$(rustup which rustc)"):$PATH"
```

## Core model

Saturn is opinionated:

- BTC-only settlement
- Lightning first
- Optional on-chain fallback
- Deterministic order state transitions
- Signed request envelopes instead of cookie or bearer-token sessions
- Nostr for discovery and public attestations, not as the transactional database

## Request model

Every mutating request carries a signed envelope:

```json
{
  "message_id": "uuid-v4",
  "timestamp": "2026-03-08T15:00:00Z",
  "nonce": "opaque-unique-string",
  "public_key": "hex-secp256k1-pubkey",
  "signature": "hex-compact-signature"
}
```

Saturn canonicalizes the JSON body, removes `signature`, hashes it, verifies the secp256k1 signature, and enforces replay protection with timestamp windows and nonce storage.

## Checkout flow

The public API surface is:

```text
GET  /capabilities
POST /quote
POST /checkout-intent
POST /payment/confirm
POST /order/:id/fulfill
GET  /order/:id
```

The primary path is:

```text
created -> quoted -> payment_pending -> paid -> fulfilled
```

## Integrations

### Settlement rails

Saturn uses `ldk-node` for live Lightning and on-chain backends. Lightning handles the fast path, while on-chain fallback covers harder settlement cases.

### Nostr

Nostr is used for merchant capability publication, quote references, and receipts. It complements Saturn's API and database; it does not replace them.

### Joinstr

Joinstr is an optional sidecar for post-settlement treasury privacy. Confirmed on-chain outputs can be handed to a Joinstr sidecar after settlement, helping merchants improve UTXO privacy without pushing CoinJoin complexity into the buyer checkout flow.

## Read next

- [Protocol specification](./protocol-spec.md)
- [Architecture overview](./architecture.md)
- [Nostr events](./nostr-events.md)
- [Repository README](../README.md)
