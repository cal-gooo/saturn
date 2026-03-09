# A2A Commerce Protocol v0.1 Draft

## 1. Goals

A2A Commerce Protocol v0.1 defines a seller-facing HTTP interface and Nostr event model for agent-to-agent BTC commerce. The design is original, neutral, and extensible:

- buyer agents request quotes and submit checkout intents
- seller agents answer with deterministic state transitions
- BTC is the only settlement asset
- Lightning is the primary rail, on-chain BTC is a controlled fallback
- Nostr anchors merchant identity, discovery, signed receipts, and relay redundancy

## 2. Trust and Identity Model

- HTTP API identity is a secp256k1 keypair controlled by the agent
- Nostr identity is the same secp256k1 public key namespace used for event publication
- seller capability documents advertise the merchant Nostr public key, relay set, rails, quote TTL, and protocol version
- signed API requests are self-authenticating and do not require session cookies or bearer tokens

## 3. Transport Model

- base transport: HTTPS JSON API
- redundancy and public attestations: Nostr
- request content type: `application/json`
- response content type: `application/json`
- correlation ID header: `x-correlation-id`
- idempotency header for mutating intent endpoints: `Idempotency-Key`

## 4. Signed Request Envelope

Every signed request body embeds the following top-level fields:

```json
{
  "message_id": "uuid-v4",
  "timestamp": "2026-03-08T14:15:22Z",
  "nonce": "opaque-unique-string",
  "public_key": "hex-encoded-secp256k1-pubkey",
  "signature": "hex-encoded-compact-signature"
}
```

Rules:

- `message_id` MUST be globally unique per signed request
- `timestamp` MUST be UTC ISO8601 and within 120 seconds of server time
- `nonce` MUST be unique per `public_key` until its replay window expires
- `public_key` MUST match the verifying secp256k1 key
- `signature` MUST verify against the canonical payload hash described below

## 5. Canonical Payload Hashing

Signature input is defined as:

1. remove the `signature` field from the JSON object
2. recursively sort object keys lexicographically
3. preserve array order exactly as sent
4. serialize the canonical JSON without insignificant whitespace
5. compute `SHA256(canonical_json_bytes)`
6. sign the 32-byte digest using secp256k1 ECDSA compact format

Rationale:

- deterministic across languages
- body-centric, protocol-neutral
- stable for future detached-signature transports

## 6. Anti-Replay Model

Replay protection combines four checks:

1. timestamp drift rejection if absolute skew exceeds 120 seconds
2. nonce uniqueness scoped to `(public_key, nonce)`
3. unique `message_id` logging for audit correlation
4. idempotency-key support on `POST /checkout-intent` and `POST /payment/confirm`

Failure modes:

- reused nonce: reject with `replay_nonce_reused`
- invalid signature: reject with `signature_invalid`
- stale timestamp: reject with `timestamp_out_of_window`
- duplicate idempotent mutation: return the original successful result if the key matches the same semantic operation; otherwise reject with `idempotency_conflict`

## 7. Privacy Model

- no PII is required on relay-published events
- order IDs and quote IDs are opaque UUIDs, not human labels
- payment receipt events contain references, rail, hashes, and finality data only
- capability discovery events are public; quote references and receipts SHOULD avoid plaintext commerce descriptions
- merchant relays can be redundant without exposing buyer metadata beyond pseudonymous Nostr keys
- future encrypted DM negotiation is compatible but out of scope for v0.1

## 8. Deterministic State Machine

Primary path:

`created -> quoted -> payment_pending -> paid -> fulfilled`

Alternative branches:

- `quoted -> expired`
- `quoted -> cancelled`
- `payment_pending -> expired`
- `payment_pending -> disputed`
- `paid -> disputed`

Transition rules:

- quote creation creates an order shell in `created`, then atomically emits `quoted`
- checkout intent can only move `quoted -> payment_pending`
- payment confirmation can only move `payment_pending -> paid`
- fulfillment is a seller-side action that moves `paid -> fulfilled`
- terminal states: `fulfilled`, `expired`, `cancelled`
- `disputed` is non-terminal but blocks automatic fulfillment

## 9. Message Types

### 9.1 `GET /capabilities`

Unsigned read-only endpoint returning:

- protocol version
- seller identity
- relay list
- supported rails
- quote TTL and lock window
- experimental Nostr event kinds

### 9.2 `POST /quote`

Signed request. Purpose:

- create a quoteable commerce request
- lock pricing parameters and settlement preferences

Payload fields:

- `buyer_nostr_pubkey`
- `seller_nostr_pubkey`
- `items[]` with `sku`, `description`, `quantity`, `unit_price_sats`
- `settlement_preference`: `lightning_only` or `lightning_with_onchain_fallback`
- `callback_relays[]`
- optional `buyer_reference`

Response fields:

- `quote_id`
- `order_id`
- `state = quoted`
- `total_sats`
- `expires_at`
- `quote_lock_until`
- `accepted_rails[]`
- `nostr_quote_reference`

### 9.3 `POST /checkout-intent`

Signed request with `Idempotency-Key`.

Payload fields:

- `quote_id`
- `selected_rail`
- optional `buyer_reference`
- optional `return_relays[]`

Behavior:

- validates quote lock window
- transitions `quoted -> payment_pending`
- produces Lightning invoice
- optionally produces fallback on-chain address

Response fields:

- `order_id`
- `quote_id`
- `state = payment_pending`
- `selected_rail`
- `lightning_invoice`
- `lightning_payment_hash`
- optional `onchain_fallback_address`
- `quote_lock_until`
- `required_onchain_confirmations`

### 9.4 `POST /payment/confirm`

Signed request with `Idempotency-Key`.

Payload fields:

- `order_id`
- `rail`
- `settlement_proof`

Settlement proof model:

- Lightning:
  - `payment_hash`
  - optional `preimage`
  - `settled_at`
  - `amount_sats`
- On-chain:
  - `txid`
  - `vout`
  - `amount_sats`
  - `confirmations`

Behavior:

- verifies payment via adapter
- enforces on-chain confirmation threshold
- transitions `payment_pending -> paid`
- persists receipt
- publishes a signed Nostr receipt event

Response fields:

- `order_id`
- `receipt_id`
- `state = paid`
- `finality`
- `receipt_event_id`

### 9.5 `GET /order/:id`

Read endpoint returning:

- `order_id`
- `quote_id`
- `state`
- `selected_rail`
- `payment_status`
- `receipt_refs[]`

### 9.6 `POST /order/:id/fulfill`

Signed request. Purpose:

- perform the seller-side fulfillment transition

Payload fields:

- `order_id` (MUST match the path id)

Behavior:

- validates signed payload and path/body order id consistency
- transitions `paid -> fulfilled`
- rejects other states with `state_transition_invalid`

Response fields:

- `order_id`
- `quote_id`
- `state = fulfilled`
- `selected_rail`
- `payment_amount_sats`
- `receipt_ids[]`

## 10. Error Codes

Structured API errors:

| Code | Meaning |
| --- | --- |
| `bad_request` | malformed request or schema mismatch |
| `schema_invalid` | JSON schema validation failed |
| `signature_invalid` | secp256k1 signature verification failed |
| `timestamp_out_of_window` | timestamp outside ±120 seconds |
| `replay_nonce_reused` | nonce already observed for same public key |
| `idempotency_missing` | required key absent |
| `idempotency_conflict` | key reused with different semantic request |
| `quote_expired` | quote or lock window expired |
| `state_transition_invalid` | requested transition is not allowed |
| `payment_verification_failed` | payment proof rejected by adapter |
| `payment_finality_pending` | on-chain proof below confirmations threshold |
| `resource_not_found` | quote, order, or receipt absent |
| `internal_error` | unclassified server error |

Error body shape:

```json
{
  "error": {
    "code": "schema_invalid",
    "message": "request body failed validation",
    "details": {
      "path": "/items/0/quantity"
    }
  }
}
```

## 11. Signature Model

- algorithm: secp256k1 ECDSA over SHA256 digest
- public key encoding: lowercase hex compressed key preferred
- signature encoding: lowercase hex compact 64-byte signature
- signature scope: canonical request body without `signature`
- response receipts are signed by the merchant server key and optionally published to Nostr

## 12. Nostr Event Strategy

Experimental kinds for v0.1:

- `31390`: replaceable merchant capability announcement
- `17390`: quote reference
- `17391`: payment receipt
- `17392`: dispute or status update

Tag strategy:

- `["t","a2ac/v0.1"]`
- `["p","<counterparty-pubkey>"]`
- `["q","<quote-id>"]`
- `["o","<order-id>"]`
- `["r","lightning"]` or `["r","onchain"]`
- `["x","<receipt-hash-or-body-hash>"]`
- `["relay","<relay-url>"]`

Capability events SHOULD use a deterministic `d` tag so the latest merchant document replaces prior versions.

## 13. Extensibility

- alternate settlement proofs can be added under new `rail` values without breaking envelope semantics
- future encrypted Nostr negotiation can wrap the same canonical signed payloads
- line items are extensible JSON objects as long as required schema remains valid

## 14. Reference Server Responsibilities

- enforce schema before business logic
- enforce deterministic state transitions
- persist replay nonces and receipts
- return structured errors with correlation IDs
- trace all state mutations with order and quote identifiers
