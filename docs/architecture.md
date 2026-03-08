# Architecture Overview

## Goals

The reference implementation is structured to keep the protocol surface stable while allowing payment, persistence, and Nostr integrations to evolve independently.

## Layers

### API layer

Located in `src/api`.

- request and response schema definitions
- JSON schema validation
- Axum handlers mapping HTTP routes to services

### Security layer

Located in `src/security`.

- canonical JSON generation
- secp256k1 request verification
- timestamp drift enforcement
- nonce replay prevention middleware

### Domain layer

Located in `src/domain`.

- order and quote entities
- payment rail and settlement proof types
- deterministic state transition rules

### Service layer

Located in `src/services`.

- quote creation
- checkout intent orchestration
- payment confirmation and receipt creation
- capability and order query assembly

### Integration layer

Located in `src/payments`, `src/nostr`, `src/persistence`, and `src/privacy`.

- Lightning and on-chain adapter traits
- Nostr publisher abstraction
- Postgres and in-memory repositories
- Optional coinjoin sidecar abstraction for post-settlement privacy workflows

## Key Design Decisions

### Signed body envelope instead of transport-coupled auth

The protocol signs canonicalized request bodies directly. This keeps the signature model portable across HTTP, Nostr, and future transports.

### Deterministic state transitions

The service layer cannot invent arbitrary order states. All transitions route through explicit state-machine checks so the protocol remains auditable.

### Swappable adapters at the edge

The payment and relay traits are designed so Saturn can move from mocks to live backends without changing the API layer. The repo now includes live Nostr publishing plus opt-in `ldk-node` adapters for Lightning and on-chain address generation/verification, while tests still use the mock path for determinism.

The same pattern now applies to treasury privacy integrations: confirmed on-chain settlements can be handed to an optional Joinstr sidecar without coupling CoinJoin concerns to quote, checkout, or payment confirmation semantics.

### In-memory repositories for fast integration tests

The test harness exercises the full checkout flow without requiring a live database. Postgres remains the runtime persistence target.

## Extension Points

- harden the `ldk-node` path with funded-node and regtest integration coverage
- replace or extend the on-chain verifier with Bitcoin Core RPC or dedicated indexer support
- add relay publishing retries and delivery observability around the live Nostr publisher
- persist Joinstr queue status and round outcomes instead of using best-effort sidecar submission
- add seller-side fulfillment endpoints without changing the request signing model
