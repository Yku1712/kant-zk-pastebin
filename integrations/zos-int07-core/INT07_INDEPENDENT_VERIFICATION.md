# INT07 independent verification

**Status: PASS for the standalone source, audit, and tests. BLOCKED for live service integration. NOT PRODUCTION-READY.**

This record documents an independent reproduction of the branch published as Draft PR #3. It is additional evidence; it does not replace the source audit or the SQLite recovery report.

## Results

| Item | Result | Evidence |
| --- | --- | --- |
| INT07-20 source audit | **PASS** | [INT07_20_CONTRACT_AUDIT.md](INT07_20_CONTRACT_AUDIT.md), independently re-read against the exact pinned commits; no synthetic production interface assumed. |
| INT07-21 typed adapter | **PASS** | `cargo fmt`, Clippy, tests, and docs pass. Fail-closed behavior is exercised by tests in `tests/publication.rs`. |
| INT07-22 SQLite durability | **PASS** | Real bundled SQLite, concurrent connections, lost response, failed dispatch, and six process-termination cases pass. |
| Evidence manifest at prior head | **FAIL (found and corrected)** | At `d53b4ca8cafbaea5cf11bbb508bb75ced5081585`, all 15 `SHA256SUMS` entries disagreed with the committed files. A byte-exact `git archive` export confirmed the manifest was stale. |
| Evidence manifest after correction | **PASS** | Regenerated from the final committed tree; verify with `sha256sum -c SHA256SUMS` on a byte-exact export. |
| Native Windows Cargo build | **BLOCKED (environment policy)** | Windows Application Control blocked Cargo-launched build scripts with `os error 4551`. A manually compiled Rust executable ran normally, so this is not a Rust source failure. |
| Isolated WSL2 Ubuntu build and tests | **PASS** | [independent validation transcript](evidence/independent_validation.log), Rust 1.95.0 as pinned by `rust-toolchain.toml`. |
| Live MCP/Pastebin/IPFS composition | **BLOCKED** | No authoritative exact-revision public-release grant, no operation-correlated durable receipt, no verified full-payload IPFS readback, and the Pastebin server needs private `nora` registry dependencies. |

## Reproduction commands

The validated commands were run against a byte-exact `git archive` export, not a CRLF-normalized Windows checkout:

```sh
sha256sum -c SHA256SUMS
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
```

Observed result: `3` unit tests passed, `8` integration tests passed, `1` child-process crash worker intentionally ignored as a direct test target, and `0` failed. The child worker is invoked six times by `process_kill_before_and_after_each_commit`.

## Scope boundaries

- No production database, deployment, protected branch, or private registry was accessed.
- All remote observations in tests are explicitly synthetic fixtures. No synthetic HTTP response is treated as an authentic Pastebin or IPFS receipt.
- The adapter never receives publication authority from artifact visibility, a share token, a witness, a local `bafk` alias, or an `ipfs_cid` field alone.
- No claim is made that SQLite and an external HTTP/IPFS service are atomically coupled. Ambiguous external operations remain `UNRESOLVED`.
