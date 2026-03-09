# OpenClaw Saturn Plugin

Use Saturn as an OpenClaw tool provider for quote, checkout, and payment confirmation.

## Tools

- `saturn_get_capabilities`
- `saturn_create_quote`
- `saturn_create_checkout`
- `saturn_confirm_payment`
- `saturn_get_order`

## Install in OpenClaw

From your OpenClaw project root:

```bash
openclaw plugins install ./apps/openclaw-saturn-plugin
```

For local-link development:

```bash
openclaw plugins install -l ./apps/openclaw-saturn-plugin
```

## OpenClaw Config Example

Copy from [`openclaw.config.example.yaml`](./openclaw.config.example.yaml) and adjust values for your environment.

```yaml
plugins:
  load:
    paths:
      - ./apps/openclaw-saturn-plugin
  entries:
    openclaw-saturn-plugin:
      enabled: true
      config:
        saturnBaseUrl: "http://127.0.0.1:3000"
        requestSigningSecretKey: "2222222222222222222222222222222222222222222222222222222222222222"
        requestTimeoutMs: 15000
        defaultCallbackRelays:
          - "wss://relay.damus.io"
        defaultSettlementPreference: "lightning_with_onchain_fallback"
        defaultSelectedRail: "lightning"

agents:
  list:
    - id: saturn-agent
      llm: openai/gpt-5
      tools:
        allow:
          - openclaw-saturn-plugin.saturn_get_capabilities
          - openclaw-saturn-plugin.saturn_create_quote
          - openclaw-saturn-plugin.saturn_create_checkout
          - openclaw-saturn-plugin.saturn_confirm_payment
          - openclaw-saturn-plugin.saturn_get_order
```

## Saturn Signing Compatibility

Mutating calls are signed in-plugin using Saturn-compatible rules:

1. Build envelope metadata (`message_id`, `timestamp`, `nonce`, `public_key`, `signature`).
2. Remove `signature` for hashing.
3. Canonicalize JSON with recursive key sorting and original array order.
4. SHA-256 hash canonical JSON bytes.
5. Sign with secp256k1 compact signature (64-byte hex).

This matches Saturn's server-side verifier in `src/security/canonical.rs` and `src/security/signing.rs`.
