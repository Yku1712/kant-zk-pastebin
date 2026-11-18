# ZOS-INT11 — Producer and verifier compatibility

**NOT PRODUCTION-READY.** The isolated original producer passed for the tested files through two 256 KiB chunks. It truncated larger inputs and reported roots after failed block writes. The proposed patch passed the corresponding isolated checks.

The Pastebin source is Git commit `ec90d56a74dbd0810fcd4849f42633386a4207f7`, also the INT10 verifier baseline. The standalone probe compiles the exact original writer and block-reader functions from `src/ipfs.rs` and invokes them in separate processes with disposable `IPFS_PATH` directories. Public package archives covering the actual encoder and codec stack matched the SHA-256 identities in the pinned Pastebin lock. Other support packages listed in the provenance log differ. The result establishes isolated producer behavior. The locked full-service build has a separate status below. [Provenance](evidence/package-provenance.log) · [Producer audit](INT11_PRODUCER_SECURITY_AUDIT.md)

| Case | Original producer and INT10 | Proposed patch and INT10 |
| --- | --- | --- |
| Empty and small file | **VERIFIED.** Original emitted DAG-PB File CIDv0 with SHA2-256. INT10 reconstructed 0 and 31 bytes exactly. | **VERIFIED.** Same CIDs and bytes. |
| First chunk boundary | **VERIFIED.** 262,143, 262,144, and 262,145 bytes all matched their authoritative SHA-256. The last case crossed into a second chunk. | **VERIFIED.** Same CIDs and bytes. |
| Two-chunk boundary | **VERIFIED.** 524,287 and 524,288 bytes reconstructed exactly. | **VERIFIED.** Same CIDs and bytes. |
| More than two chunks, within verifier budget | **FAIL, VERIFIED.** At 524,289, 786,449, and 1,048,576 bytes the original reported the same CID as the common 524,288-byte prefix. INT10 rejected the full-input digest and reconstructed the prefix only when given its digest. | **VERIFIED.** INT10 reconstructed all inputs and matched each full authoritative digest. |
| Default nested input, 45,613,057 bytes | **FAIL, VERIFIED.** The original stored the same two-chunk prefix and offline Kubo confirmed only 524,288 bytes. The resulting graph had no depth-two links. | **VERIFIED by offline Kubo.** The patched default writer emitted depth-two links and Kubo reconstructed all 45,613,057 bytes. **INT10 rejected the complete input** at its intentional 1 MiB output limit. |
| Missing repository and obstructed flatfs path | **FAIL, VERIFIED.** The original reported a CID. An obstructed store could not supply the claimed bytes. | **VERIFIED.** Both failures returned `None`. |
| Partial persistence with a failed later leaf | **FAIL, VERIFIED.** The first leaf and root were present while a later leaf path was blocked. INT10 rejected the unreadable child. | **VERIFIED.** The first leaf remained, and the writer withheld the root. |
| Removed or truncated child of a valid stored file | **VERIFIED.** INT10 returned `MissingBlock` or `HashMismatch` and reconstructed correctly after restoring the child. | **VERIFIED.** Same fail-closed verification. |
| Raw CIDv1 and small nested UnixFS from Kubo | **VERIFIED for INT10 on offline Kubo-generated fixtures.** These fixtures were generated separately from Pastebin. The audited default Pastebin writer emits CIDv0 DAG-PB leaves and parents. | Same verifier coverage. |
| Running Pastebin service and production storage | **BLOCKED.** Authentic full-service dependency resolution and local deployment were unavailable within scope. | **BLOCKED.** The patch is experimental and the service requires its own authenticated build and integration validation. |
| Live IPFS routing, publishing, pinning and remote availability | **NOT TESTED.** | **NOT TESTED.** |

The exact root CIDs, reconstructed sizes, dependency and toolchain output, and command exit statuses are recorded in the [Cargo transcript](evidence/complete-validation.log). The independent verifier's inherited Kubo fixtures and its INT07 integration test passed again. Four new compatibility tests passed against the original writer and the same four passed against the patch. A passing regression test on the original can mean it reproduced a known defect.

## Reproduction

From the repository root, with Rust 1.95.0 and a C compiler, build the probes separately using their public lock and run the compatibility suite with explicit mode and binary paths. The optional `INT11_KUBO` variable points to an authentic Kubo binary. The recorded run used Kubo v0.36.0. Each test initializes its own repository and invokes Kubo in offline mode.

```sh
base=ec90d56a74dbd0810fcd4849f42633386a4207f7
core=integrations/zos-int07-core/agent2
probe="$core/int11/probe/Cargo.toml"
verifier="$core/unixfs/Cargo.toml"
scratch=$(mktemp -d)
mkdir "$scratch/patched"
git archive "$base" src/ipfs.rs Cargo.toml | tar -x -C "$scratch/patched"
git -C "$scratch/patched" apply "$PWD/$core/int11/INT11_FAIL_CLOSED_PRODUCER.patch"

cargo build --locked --manifest-path "$probe" --target-dir "$scratch/original-target"
INT11_PASTEBIN_SOURCE="$scratch/patched/src/ipfs.rs" cargo build --locked \
  --manifest-path "$probe" --target-dir "$scratch/patched-target" --features patched-writer

INT11_MODE=pinned INT11_PROBE="$scratch/original-target/debug/pastebin-writer-probe" \
  cargo test --locked --manifest-path "$verifier" --all-features --test pastebin_producer_compat -- --ignored --nocapture
INT11_MODE=patched INT11_PROBE="$scratch/patched-target/debug/pastebin-writer-probe" \
  cargo test --locked --manifest-path "$verifier" --all-features --test pastebin_producer_compat -- --ignored --nocapture
```

Run `sha256sum -c SHA256SUMS` inside `agent2/int11/` to check the exact review bytes, including the separate verifier test file. Review records and lockfiles are linked in the [audit](INT11_PRODUCER_SECURITY_AUDIT.md).
