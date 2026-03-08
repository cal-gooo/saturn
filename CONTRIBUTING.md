# Contributing

## Scope

This repository is the reference implementation for A2A Commerce Protocol v0.1. Changes should preserve three constraints:

- BTC-only settlement
- Nostr-native identity and receipt model
- protocol neutrality with no ACP code reuse

## Development Setup

1. Install Rust stable with `rustfmt` and `clippy`.
2. Start Postgres:

```bash
docker compose up -d postgres
```

3. Copy local configuration:

```bash
cp .env.example .env
```

4. Run checks before opening a pull request:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

## Change Expectations

- Keep protocol changes documented in [docs/protocol-spec.md](docs/protocol-spec.md).
- Add or update tests for signing, replay prevention, state transitions, and checkout flows when behavior changes.
- Prefer additive protocol evolution over breaking wire-format changes.
- Keep Nostr relay payloads free of PII.
- Use structured API errors instead of ad hoc strings.

## Pull Requests

- Keep PRs narrowly scoped.
- Explain protocol impact, storage impact, and compatibility impact.
- Include example request and response payloads when API behavior changes.
- If a schema changes, update docs and test coverage in the same PR.

## Commit Guidance

- Use imperative commit messages.
- Avoid mixing refactors with behavior changes unless the refactor is required for the fix.
