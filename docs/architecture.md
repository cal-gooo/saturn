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

Located in `src/payments`, `src/nostr`, and `src/persistence`.

- Lightning and on-chain adapter traits
- Nostr publisher abstraction
- Postgres and in-memory repositories

## Key Design Decisions

### Signed body envelope instead of transport-coupled auth

The protocol signs canonicalized request bodies directly. This keeps the signature model portable across HTTP, Nostr, and future transports.

### Deterministic state transitions

The service layer cannot invent arbitrary order states. All transitions route through explicit state-machine checks so the protocol remains auditable.

### Mock adapters at the edge

The current payment and relay implementations are mocks, but the traits are shaped to support real backends without changing the API layer.

### In-memory repositories for fast integration tests

The test harness exercises the full checkout flow without requiring a live database. Postgres remains the runtime persistence target.

## Extension Points

- replace mock Lightning adapter with LND, Core Lightning, or LNbits integration
- replace mock on-chain adapter with Bitcoin Core RPC or indexer-backed settlement checks
- replace mock Nostr publisher with live multi-relay publishing and delivery retries
- add seller-side fulfillment endpoints without changing the request signing model

