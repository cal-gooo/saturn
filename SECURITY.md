# Security Policy

## Supported Versions

The `main` branch is the supported development line until tagged releases are established.

## Reporting

Do not open public GitHub issues for signing flaws, replay bypasses, payment verification gaps, or key-handling vulnerabilities.

Report security issues privately through GitHub security advisories for this repository. If advisory tooling is unavailable, contact the maintainer directly before publishing details.

## Areas Requiring Extra Care

- secp256k1 signing and canonical payload hashing
- nonce replay prevention and timestamp validation
- idempotency semantics for payment mutations
- on-chain confirmation handling
- Nostr receipt privacy and metadata leakage
- SQL queries and persistence of settlement proofs

## Disclosure Expectations

- Provide a minimal reproduction or proof-of-concept if possible.
- Allow time for assessment and remediation before public disclosure.
- Coordinated disclosure is preferred.

