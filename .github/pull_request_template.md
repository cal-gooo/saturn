## Summary

- describe the change
- describe the protocol or API impact

## Validation

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test --all-targets`

## Protocol Checklist

- [ ] wire contract updated if needed
- [ ] Nostr event impact reviewed
- [ ] replay/signature semantics unchanged or documented
- [ ] migrations included for persistence changes

