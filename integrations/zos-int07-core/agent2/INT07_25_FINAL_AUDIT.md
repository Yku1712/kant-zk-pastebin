# INT07-25 — Independent final audit

**NOT PRODUCTION-READY**

| Deliverable | Result |
| --- | --- |
| INT07-23 defensive validation | **VERIFIED** against frozen Rust code. The 11 original tests and 8 independent tests pass. Source authorization and IPFS error observations use synthetic providers. |
| INT07-24 patch | **VERIFIED** against the actual standalone Rust core and a separate local SQLite store. Three integration tests pass. The patch applies cleanly to the frozen candidate and adds no production routes. |
| Live integration | **BLOCKED** on authoritative source release, authentic Pastebin dependencies, a durable service-side operation/receipt contract, and full verified UnixFS readback. |
| Physical durability, operating system enforcement and deployed authorization | **NOT TESTED**. The proven recovery cases involve process termination and SQLite transaction rollback/reopen semantics. |
| Evidence integrity | **VERIFIED** for the frozen candidate's 15 manifest entries. **FAIL** for the subsequent Agent 1 base manifest as committed. All 17 entries match only after CRLF conversion. The Agent 2 branch regenerates the manifest over exact Git file bytes and adds its own deliverables. |

Frozen candidate `d53b4ca8cafbaea5cf11bbb508bb75ced5081585` was fetched and independently validated before any change. PR #3 had moved to `8217990a110cbfa96aae7dfd149a65ab4ddc0120` at branch creation. A Git diff confirmed the Rust source, original tests, dependency manifests and lockfile match the frozen candidate. Agent 2's branch starts from the newer reviewed head to keep the stacked review scoped. PR #3 and its branch are preserved.

The latest Agent 1 verification note reports that the frozen manifest failed and the newer one passed. A byte-exact Linux checkout and frozen export produce the opposite results. The proof in the [manifest comparison](evidence/manifest-comparison.log) checks both Git bytes and a hypothetical CRLF conversion. This review's manifest uses Git bytes. The original note remains available in the source history.

## Reproduction

Run from `integrations/zos-int07-core/` using the pinned Rust toolchain and a C compiler. The patch is already represented as files in this review branch. Apply it only to a separate checkout of the frozen candidate when validating the exchange artifact.

```sh
sha256sum -c SHA256SUMS
cargo build --locked
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo fmt --all --manifest-path agent2/experimental/Cargo.toml --check
cargo build --locked --manifest-path agent2/experimental/Cargo.toml
cargo clippy --all-targets --locked --manifest-path agent2/experimental/Cargo.toml -- -D warnings
cargo test --locked --manifest-path agent2/experimental/Cargo.toml
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked --manifest-path agent2/experimental/Cargo.toml
```

Evidence files record the [frozen run](evidence/frozen.log), [expanded run](evidence/review.log), and [clean patch application and tests](evidence/patch.log). See the [adversarial report](INT07_23_ADVERSARIAL_REPORT.md), [integration design](INT07_24_INTEGRATION_DESIGN.md), and [primary agent handoff](HANDOFF_PRIMARY.md).
