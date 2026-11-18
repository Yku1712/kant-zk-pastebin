# ZOS-INT-08 authorized Artifact MCP bridge

**ACCEPT FOR EXPERIMENTAL REVIEW. NOT PRODUCTION-READY. Live integration BLOCKED.**

This isolated crate implements a typed `zos_int07_core::AuthorizedSource` bridge. It requires an
authoritative, consistent exact-revision snapshot and two distinct capabilities: a read grant and
an explicit public-release grant. It never derives authority from raw `ArtifactMeta`, visibility
flags, share tokens, or caller-supplied metadata.

The real source audit is [AUTHORITY_CONTRACT.md](AUTHORITY_CONTRACT.md). The fail-closed analysis is
[SECURITY_ANALYSIS.md](SECURITY_ANALYSIS.md). Validation output is in
[evidence/validation.log](evidence/validation.log).

## Status

| Item | Result |
| --- | --- |
| Actual Artifact MCP source audit at `731d5b6e…` | PASS |
| Typed authority bridge | PASS |
| Ten adversarial tests | PASS |
| Format, Clippy, tests, docs under Rust 1.95.0 | PASS |
| Evidence manifest | PASS |
| Live Artifact MCP adapter | BLOCKED: no exported authoritative grant/release boundary |
| Production readiness | NOT PRODUCTION-READY |

## What is implemented

- `AuthenticatedPrincipal`: private fields, issued only by `CredentialVerifier`.
- `AccessPolicy` / `ReleaseAuthority`: source-side grant-issuing model with distinct read and release
  capabilities.
- `AuthoritySnapshot`: private, consistency-bound snapshot of one exact revision.
- `AuthoritativeSource`: the production port the real Artifact MCP boundary must implement.
- `ArtifactMcpBridge`: fail-closed `AuthorizedSource` for INT07's `Publisher`.
- `ContractCompatibleProvider`: explicitly synthetic in-memory provider used only by tests.

`zos-int07-core` is a path dependency and was not modified by this change.

## Reproduce

Run from this directory after a byte-exact checkout. Rustup, a C compiler, and the committed
`Cargo.lock` are required.

```sh
sha256sum -c SHA256SUMS
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
```

Observed validation: `12` adversarial tests passed, `0` failed. No network or live service was used.
`evidence/toolchain.txt` records the exact compiler, dependency tree, and host information.

## Required production boundary still missing

Artifact MCP's `AuthorizedArtifact` grants are minted only by sanctioned `AccessPolicy` constructors
from an authenticated principal and a live metadata lookup. `ArtifactPublicView` is a non-serializable
display snapshot and explicitly not an authorization witness. There is no first-class public-release
permission; the nearest gate is `artifacts:visibility` plus owner/org policy.

A live bridge therefore needs an Artifact MCP-side adapter that re-enters the existing authenticated
path and returns an exact-revision authoritative snapshot with an explicit release decision. This
review stops at the typed interface and does not claim live integration.
