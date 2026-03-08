# Nostr Event Schema

This document defines the v0.1 event contract used by the reference implementation. Events intentionally avoid personally identifying information and carry only routing-safe metadata.

## Capability Announcement

- `kind`: `31390`
- replaceable by merchant using `["d","merchant-capabilities"]`
- content: compact JSON capability document

Tags:

- `["d","merchant-capabilities"]`
- `["t","a2ac/v0.1"]`
- `["relay","wss://relay.damus.io"]`
- `["relay","wss://nos.lol"]`

## Quote Reference

- `kind`: `17390`
- references a generated quote without publishing basket details

Tags:

- `["t","a2ac/v0.1"]`
- `["q","<quote-id>"]`
- `["o","<order-id>"]`
- `["p","<buyer-pubkey>"]`
- `["p","<seller-pubkey>"]`
- `["x","<canonical-body-hash>"]`

## Payment Receipt

- `kind`: `17391`
- signed by merchant
- proves that a particular order reached `paid`

Tags:

- `["t","a2ac/v0.1"]`
- `["o","<order-id>"]`
- `["q","<quote-id>"]`
- `["r","lightning"]`
- `["x","<receipt-hash>"]`
- `["p","<buyer-pubkey>"]`

## Dispute / Status Update

- `kind`: `17392`
- used for `fulfilled`, `cancelled`, `expired`, `disputed`

Tags:

- `["t","a2ac/v0.1"]`
- `["o","<order-id>"]`
- `["s","fulfilled"]`
- `["p","<buyer-pubkey>"]`
- `["p","<seller-pubkey>"]`

## Content Rules

- event `content` SHOULD be minified JSON
- event `content` MUST omit names, email addresses, delivery coordinates, or fiat references
- event `content` MAY include hashes, timestamps, rail metadata, and finality statements

