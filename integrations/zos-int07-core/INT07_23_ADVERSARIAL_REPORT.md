# INT07-23 — Independent adversarial Rust audit

**Verdict: ACCEPT WITH CORRECTIONS for experimental review. NOT PRODUCTION-READY.**

The implementation under test is `8217990a110cbfa96aae7dfd149a65ab4ddc0120` from Draft PR #3. The baseline ran before repo edits in a detached checkout and a fresh Linux Cargo target directory. Git verified that production Rust, the original publication tests and the Cargo manifests remained byte-identical to the pin for the expanded run.

| Check | Result |
| --- | --- |
| Baseline SHA-256 manifest | **FAIL.** All 17 entries mismatch raw Git bytes. All 17 match when those bytes are converted to CRLF. See the [byte comparison](agent2/evidence/manifest-comparison.log). |
| Baseline Rust verification | **PASS.** Format, Clippy, 3 unit tests, 8 integration tests, and documentation. |
| Expanded adversarial verification | **PASS.** 12 dedicated integration tests. The full standalone run passed 23 tests with zero failures. The two ignored Rust test functions are subprocess entry points called by their parent tests. |
| Corrected manifest | **PASS.** Regenerated over exact file bytes and verified on an export of the review commit. |
| Authentic service composition | **BLOCKED.** The audit used real Rust and SQLite, with synthetic Artifact MCP authorization, release, destination, and IPFS evidence. Live proofs and authentic Pastebin build dependencies were unavailable. |
| Power loss, disk exhaustion and live provider failures | **NOT TESTED.** |

## Security assertions

| Case | Result | Observation |
| --- | --- | --- |
| A. Conflicting identity | PASS | A different authorized digest for the same source, tenant, artifact, revision, and destination returned `JournalConflict` after the original confirmed. Reopening preserved the original receipt, the alternate had no row, and submit count stayed one. |
| B. Tenant authorization | PASS | An unauthenticated principal, stale fixture grant, wrong-tenant grant, or mismatched authoritative tenant failed before publication and without an operation row. |
| C. Revocation | PASS | The source snapshot still classified the body public. Revoking the separate eligibility policy caused replay to return `Ineligible` before lookup or dispatch. The authorized journal inspector could still see the original historical confirmation. |
| D. Concurrent dispatch | PASS | Two independent journal connections overlapped. An identical contender observed UNRESOLVED while a different-digest contender returned `JournalConflict`. The winner confirmed and one submit was observed. |
| E. Process recovery | PASS | The frozen unit suite terminated six child processes before and after the prepare, claim, and confirm commits. The new suite explicitly checked exit signal 9 in three more cases, then checked `PRAGMA integrity_check` and persisted state after reopening. It killed after claim, during lookup reconciliation before confirmation, and after confirmation. The two pre-confirmation cases remained UNRESOLVED. |
| F. Receipt validation | PASS | A different operation, altered witness, malformed ID, wrong alias and malformed CID each returned `InvalidReceipt`. Missing and partial IPFS evidence returned `ExternalUnavailable` and `DigestMismatch` respectively. Every rejected operation remained UNRESOLVED. |
| G. Storage errors | PASS | Real bundled SQLite returned `NotADatabase`, `CannotOpen`, or `DatabaseBusy` in disposable files. Writer contention did not create a row or call the destination. |
| H. Ambiguous external evidence | PASS | The original lost-ack test confirmed through later fixture readback. The default retry hook issued no repeat submission on a lookup miss. Reconciliation interrupted before evidence returned left the operation UNRESOLVED. |

Run from the crate directory with Rust 1.95.0 and a C compiler. The full combined stdout/stderr transcript records each command and exit code.

```sh
sha256sum -c SHA256SUMS
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked --test adversarial_security_suite
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
```

[Defensive tests](tests/adversarial_security_suite.rs) · [Execution transcript](evidence/independent_test.log) · [Toolchain](evidence/toolchain.txt) · [Findings and handoff](INT07_23_FINDINGS.md)
