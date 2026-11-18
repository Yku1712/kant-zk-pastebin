# ZOS-INT-07 standalone Rust core

**Experimental. NOT PRODUCTION-READY.**

| Stage | Result | Evidence |
| --- | --- | --- |
| INT07-20 | **PASS** | [Commit-pinned Rust source audit](INT07_20_CONTRACT_AUDIT.md) |
| INT07-21 | **PASS** | Standalone typed contracts and fail-closed publication with synthetic providers |
| INT07-22 | **PASS** | [SQLite recovery report](INT07_22_SQLITE_RECOVERY.md), including subprocess termination before and after commit |
| Live service integration | **BLOCKED** | Explicit public-release authority, durable provider operation correlation, and verified UnixFS readback are missing. The Pastebin server build requires private `nora` dependencies. |

The crate has no server or production wiring. It requires an authorized snapshot of one exact single-file body, explicit public-release classification, destination-specific policy approval and matching SHA-256 before journaling or sending. Receipt confirmation requires a trusted proof-of-origin and independent byte readback. IPFS confirmation additionally requires verified block traversal and full payload equality for UnixFS, or hash verification for raw blocks. The local spool alias and full witness have separate types and purposes.

## Reproduce the evidence

Run from this directory using Rustup and a C compiler for bundled SQLite. See [toolchain and dependencies](evidence/toolchain.txt) and the captured [validation transcript](evidence/validation.log).

```sh
sha256sum -c SHA256SUMS
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
```

The isolated workspace uses its own lockfile and pulls its dependencies from crates.io. Reproducibility requires access to those public packages or a matching Cargo cache. The Apache-2.0 license is included. The audit cites upstream code and the implementation uses original code.
