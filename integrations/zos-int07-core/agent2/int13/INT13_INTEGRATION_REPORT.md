# ZOS-INT13 — Independent isolated end-to-end integration

**NOT PRODUCTION-READY.** Eight adversarial end-to-end tests passed. The tested source authority is synthetic. IPFS block production, verification, and both SQLite transactions use the real pinned experimental implementations.

## Source and dependency matrix

| Component | Exact reviewed commit | Runtime dependency and use |
| --- | --- | --- |
| INT07-26, PR #6 | `dc748bb007e458bb88fb4b24647e40809b188d5c` | `zos-int07-core 0.1.0`, path dependency. Uses the real streaming `Publisher`, journal, SHA-256 intent binding, and explicit durable retry interface. |
| INT08, PR #5 | `a9dde4d712dc0c99c3ef2b30d92ef49723695c77` | `zos-int08-authorized-artifact-bridge 0.1.0`, imported library, manifest, and Apache license as exact Git blobs. Uses `CredentialVerifier`, `ContractCompatibleProvider`, and `ArtifactMcpBridge`. |
| INT10, PR #7 | `ec90d56a74dbd0810fcd4849f42633386a4207f7` | `zos-unixfs-readback 0.1.0` with the `int07` feature. Uses `Flatfs`, actual DAG verification and the INT07 receipt-evidence wrapper. |
| INT11, PR #8 | `8cb627b41bd53a555557d7e46a5c1c1070d15dde` | `pastebin-writer-probe 0.1.0` built with its own lock. Applies the exact INT11 fix to the audited writer in a clean export, then executes the result as a subprocess. `rust-unixfs 0.5.1`, `ipld-dagpb 0.2.2`, `quick-protobuf 0.8.1`, and thirteen further version/archive identities match the pinned Pastebin lock. The isolated support package differences are recorded in the producer transcript. |
| INT12, PR #9 | `361fae9395e4c7a18bbecf0099f2c613cc785d6d` | `zos-ipfs-local-durability 0.1.0`, path dependency and direct branch base. Promotes the graph, verifies readback, and commits the independent destination ledger. |
| INT09 | Local patch-only | **BLOCKED for independent validation.** The harness uses the explicitly labeled test authority. |

The harness lock resolves public `cid 0.11.3`, `sha2 0.10.9`, `rusqlite 0.37.0` with bundled `libsqlite3-sys 0.35.0`, `tempfile 3.27.0`, and `data-encoding 2.11.1`. Rust and Cargo were 1.95.0 on Linux x86-64 and tests operated on OverlayFS. The pinned INT07-26, INT10, INT11 and INT12 source files remained byte-identical. [Baselines transcript](evidence/baselines.log) · [Probe build and checksum verification](evidence/producer.log) · [Harness lock](Cargo.lock)

**INT08 evidence integrity.** All 13 entries in the upstream INT08 manifest mismatched exact Git bytes and matched after CRLF conversion. The imported three files are unchanged Git blobs. The INT13 manifest records their exact SHA-256 hashes. [Pinned manifest comparison](evidence/int08-manifest.log)

## Composition and trust boundaries

1. INT08 authenticates a fixture credential, enforces the tenant and issues authorization for an exact revision and digest through its existing snapshot and grant checks. INT13 also supplies a synthetic destination-scoped public lease with explicit expiry and revocation.
2. INT07 checks authorization, eligibility, digest and the 1 MiB size ceiling. It commits an UNRESOLVED bridge dispatch claim before calling the destination.
3. The destination runs the corrected INT11 writer in a separate staging directory for each attempt. INT12 promotes actual candidate blocks, synchronizes them, and verifies authoritative length and SHA-256 with INT10 before committing its local graph-completion receipt.
4. The bridge reads back the receipt independently of the submit response, requires the matching operation, witness and CID, and reconstructs the bytes from local blocks. `UnixfsReceiptEvidence` independently verifies the same graph. These checks use one local content store and independent readback calls. Live receipt provenance and a distinct service readback path remain unverified.
5. INT07 rechecks release and commits its confirmation in its own database. The INT13 return boundary also verifies current destination storage before returning a historical confirmation. The destination lookup and receipt report local completion only.

The journal and destination ledger are separate SQLite databases. The two commits have an observable recovery window. Reconciliation uses the same operation identity across both databases. No atomic cross-database transaction is claimed. The durable retry capability is limited to the managed INT12 destination. [Harness implementation](src/lib.rs)

## Adversarial results

| Scenario | Verified result |
| --- | --- |
| Unauthorized tenant, expired or revoked release, revoked read grant | Refused before bridge admission or writer staging, with zero destination receipts. |
| Wrong revision, substituted snapshot revision, and wrong digest | INT08 rejected the request or snapshot. No operation persisted. |
| Altered or missing genuine staged UnixFS child | INT10/INT12 rejected the graph. Bridge remained UNRESOLVED and the destination ledger had no receipt. |
| SIGKILL before destination COMMIT | Root block was already in the canonical path and the child read was paused. The destination had no receipt. After reopen, lookup-only left the bridge UNRESOLVED. An explicit retry produced one verified receipt. |
| SIGKILL after destination COMMIT | Child stopped before returning a submit acknowledgement. Destination receipt survived, bridge remained UNRESOLVED, and lookup-only recovered confirmation without a second staging attempt. |
| Lost acknowledgement | A fixture I/O outcome was injected only after a real destination commit. Reconciliation found the actual receipt and completed without staging again. |
| Concurrent identical operations | An overlapping lookup-only call observed UNRESOLVED. An explicitly requested concurrent retry overlapped the initial stage. Both returned the same bridge confirmation and the destination contained one receipt. |
| Conflicting content for one revision | A second, individually authorized fixture snapshot with a different digest conflicted in INT07. The first receipt remained intact and no second operation staged. |
| Storage corruption after confirmation | Destination lookup and the harness return boundary rejected the damaged graph. The immutable historical bridge row remained CONFIRMED and the writer did not run again. |
| Publication above 1 MiB | INT07 returned ResourceLimit before creating a journal row or staging content. |
| Expiry and revocation during destination promotion | Both put the bridge in UNRESOLVED and withheld confirmation. The provider had already accepted the request and committed one local receipt. Continuing to retry with the revoked or expired grant was denied. |

**Crash evidence.** Two subprocesses terminated by signal 9. Both SQLite databases passed integrity checks after reopening. The pre-commit root was recorded as an orphan until replay. Each killed process left its staging directory behind. The INT12 inventory covers the managed flatfs repository. An operator would need a separate staging retention and cleanup policy. [Source tests](src/tests.rs) · [Full command and exit transcript](evidence/validation.log)

## Security findings and open boundaries

- **Live authority remains BLOCKED.** INT08 is the actual contract implementation, and the test credential service and release lease are synthetic. INT09 was outside this independent build. A live system needs authenticated source and release authority, with a guard or sender/receiver acceptance protocol that defines what happens when authorization changes during publication.
- **Revocation cannot undo an accepted local side effect.** The harness observes revocation before the provider's commit while the graph is in flight. INT07 withholds its bridge confirmation, but INT12 can finish an already accepted local operation. The local receipt remains evidence of that effect and must be reconciled under the applicable release policy.
- **Current verification and historical confirmation are distinct.** The raw INT07 journal retains historical CONFIRMED state after the underlying graph is corrupted. The INT13 access boundary must check INT12 before reporting a currently usable result. A reconcile/repair workflow is still needed for corrupted committed storage.
- **Enforce admission at upstream allocation.** INT07 rejects publication over its shared 1 MiB ceiling before journaling or staging. INT08's fixture can already have cloned and hashed the body by that point. Deployments need an upstream read or decompression budget and a managed staging cleanup/cancellation policy.
- **Full Pastebin integration and remote publication remain outside scope.** The isolated producer executes exact audited source with verified public encoder archives and a separate lock. The authenticated application build, deployment, remote receipt provenance, and live IPFS network availability are BLOCKED or NOT TESTED. [INT11 service evidence](../int11/evidence/service-blocker.log)

**INT08 test API drift.** All 12 original adversarial tests passed when run against the exact PR #5 tree. The standalone PR #5 test fixtures use the former vector-returning `ReceiptEvidence` signatures. Running those exact tests against the INT07-26 core produced two Rust E0050 signature errors. The actual INT08 library builds unchanged in the INT13 harness. [Incompatible fixture build](evidence/int08-contract.log) · [Original pinned suite](evidence/int08-original-tests.log)

## Reproduction

From the review checkout root, with Rust 1.95.0 and a C compiler:

```sh
dir=integrations/zos-int07-core/agent2
pin=8cb627b41bd53a555557d7e46a5c1c1070d15dde
scratch=$(mktemp -d)
mkdir "$scratch/writer"
git archive "$pin" Cargo.toml src/ipfs.rs | tar -x -C "$scratch/writer"
git -C "$scratch/writer" apply "$PWD/$dir/int11/INT11_FAIL_CLOSED_PRODUCER.patch"
INT11_PASTEBIN_SOURCE="$scratch/writer/src/ipfs.rs" cargo build --locked \
  --manifest-path "$dir/int11/probe/Cargo.toml" --features patched-writer --target-dir "$scratch/probe"
(cd "$dir/int13" && sha256sum -c SHA256SUMS)
INT13_PROBE="$scratch/probe/debug/pastebin-writer-probe" cargo test --locked \
  --manifest-path "$dir/int13/Cargo.toml" -- --ignored --skip tests::crash_child --nocapture
```

The standalone harness built and passed formatting, Clippy and documentation checks. The eight main tests all passed. The private subprocess entry runs only through its crash parent.
