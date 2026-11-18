# ZOS-INT12 — IPFS storage durability and failure atomicity

**NOT PRODUCTION-READY.** The isolated Linux protocol commits a local graph-completion receipt only after synchronizing and independently reading back the stored blocks. Process-crash recovery, retry integrity, and failure handling passed in the test environment. Actual power-loss durability and live service behavior remain unverified.

## Pinned inputs and claims

| Area | Classification | Evidence |
| --- | --- | --- |
| Authenticated producer | **VERIFIED in isolation** | The original writer came from `8cb627b41bd53a555557d7e46a5c1c1070d15dde`, with `src/ipfs.rs` SHA-256 `fa26cf8b2ccdd2aeabe3324efc1ccacd8d13becf660d7b743eb62e16182c9b3e`. Applying its INT11 patch to a clean export produced `96d4df2a8a2bf9984216cbdfb077e5d0420d51b61cbb43e114b69325f420b68f`. Both probes rebuilt with the INT11 public lock and 16 relevant archive hashes matched the original lock. The original and patched probes retain the isolated dependency differences recorded in INT11. [Build and archive provenance](evidence/producer.log) |
| INT11 temporary write and rename durability | **FAILED for explicit synchronization** | Source review found `write_all` followed by `persist` without file or directory sync. A real syscall trace observed `renameat` and zero `fsync` or `fdatasync` calls for the INT11 fixture. [INT11 patch](../int11/INT11_FAIL_CLOSED_PRODUCER.patch) · [INT11 syscall trace](evidence/int11-syscalls.log) |
| INT12 file and directory ordering | **VERIFIED on Linux OverlayFS** | The trace observed temporary file `fsync`, hard-link installation, temporary unlink, shard `fsync`, blocks-parent `fsync`, and then SQLite commit. SQLite used rollback journal DELETE and synchronous EXTRA. The successful receipt transaction included a database-directory `fsync` after journal unlink. [INT12 syscall trace](evidence/int12-syscalls.log) |
| Readback, process crashes, overlap and immutable retries | **VERIFIED in the isolated store** | Authentic UnixFS blocks, real disk paths, real bundled SQLite, concurrent connections, and signal 9 subprocess exits. [Execution log](evidence/validation.log) |
| Power failure, disk exhaustion at the device, kernel/filesystem stalls, cross-host storage, and concurrent unmanaged writers | **NOT TESTED** | The tests killed only a userspace process on OverlayFS. They checked observable sync operations, state, and receipt validity following reopen. |
| Full Pastebin service and remote publication | **BLOCKED** | The authenticated full service dependency and deployment path from INT11 remains unavailable. Pinning, routing and network availability were **NOT TESTED**. [INT11 service blocker](../int11/evidence/service-blocker.log) |

## Experimental completion protocol

1. Start with independently authorized INT07 intent, exact input length and expected content SHA-256. Bind the local store to a stable destination identifier. Produce a candidate in a separate repository using the exact patched INT11 writer. Treat candidate output as uncommitted.
2. Acquire SQLite's `BEGIN IMMEDIATE` writer lock. Serialize all managed promotion and orphan inventory across connections. Reject a conflicting root or authoritatively different digest for an already committed revision. A successful idempotent replay re-verifies committed disk bytes and returns the identical receipt.
3. Read the staged DAG via INT10 and hash each encoded block before promotion. Write a sibling temporary file, synchronize that file, hard-link it to the canonical flatfs path without replacement, remove the temporary name and synchronize the shard directory. For existing blocks, require byte equality and synchronize the existing file and directory. A differing or malformed canonical entry fails closed. A sync, filesystem, or verification error aborts the receipt transaction.
4. Synchronize the blocks parent directory so newly created shard entries are covered. Independently reconstruct the graph again from the managed canonical paths, checking the exact length and digest. Commit the receipt and all referenced multihash keys in the same SQLite transaction. Local `lookup` re-verifies the committed graph and only returns a receipt for a valid, complete graph.
5. Reconcile uncertain replies using the same operation ID and local lookup. The authoritative commit point is the SQLite transaction, under the managed-store trust assumptions. A CID-shaped file can exist before its children finish while a receipt remains absent. The store protects graph completion at its receipt boundary. Physical storage and remote publication are separate authority domains.

The repository root and its parent are synchronized on initialization. The managed filesystem and its ancestors must be trusted, local, and pre-provisioned. Durable operation semantics assume SQLite and the filesystem honor their documented sync and locking contracts. Operators must exclude external block writers and GC from the managed repository. The prototype exposes no HTTP route, service integration or remote authority. Observer callbacks used for fault injection are private test seams. [Implementation](src/lib.rs)

## Recovery matrix

The fixture was an authentic three-block UnixFS graph with the root visited first. At each subprocess marker, a second SQLite connection checked receipt visibility while the child was alive. The test then required exit signal 9 and checked database integrity, foreign keys, receipt visibility and canonical paths after reopening.

| Boundary | SIGKILL and reopen outcome | Injected I/O error and unwind |
| --- | --- | --- |
| Before first write | No receipt, no blocks or temporary files. | No receipt or retained blocks. |
| Halfway through the temporary write | No receipt. One half-written temporary file, no canonical block. | No receipt, temporary file removed. |
| After temporary file sync, before hard link | No receipt. One full temporary file, no canonical block. | No receipt, temporary file removed. |
| After hard link, before shard sync | No receipt. One canonical orphan and one temporary link. | No receipt. One canonical orphan, temporary link removed. |
| After shard sync | No receipt. One canonical orphan, no temporary file. | No receipt. One canonical orphan. |
| After all disk checks, before SQLite COMMIT | No receipt. All three blocks counted as unreferenced. | No receipt. All three blocks counted as unreferenced. |
| After SQLite COMMIT, before acknowledgement | Receipt and reference rows recover. All blocks verify. | Caller sees error and lookup recovers the committed receipt. |

**VERIFIED.** A clean retry completed every interrupted case and preserved the inode of each fixture root already at the canonical path. Fault callbacks returned injected I/O errors at the indicated boundaries. A separate obstruction used a real directory where a new leaf file was expected. That failure preserved an earlier committed receipt and root inode, while the later root remained unreferenced until successful replay. Deliberate post-commit corruption caused lookup and retries to reject the data while leaving the altered file untouched. Concurrent writers received one receipt for duplicated intent and distinct receipts for different content. Two graphs shared one leaf across five unique block keys, and a conflicting same-revision digest was rejected. [Fault and concurrency tests](src/tests.rs) · [Validation transcript](evidence/validation.log)

## Orphans and size limits

**VERIFIED.** The inventory runs under the writer lock and separates unreferenced canonical SHA2-256 files from abandoned temporary names. It computes its reference set from durable SQLite entries. The test distinguishes abandoned half-written files, fully installed unreferenced blocks, and blocks shared by committed receipts.

**SOURCE-REVIEWED GC procedure.** Use a dedicated repository and freeze promotions through the same exclusive lock. Back up the ledger. Reconstruct and verify every committed root and independently mark the actual reachable multihashes. Cross-check reachability against the receipt reference table. Stop on unknown roots, corruption, incomplete backup, or any mismatch. Quarantine only unmarked canonical files and confirmed abandoned temporary names while holding the lock. Allow a recovery window, re-check reachability before deletion, and synchronize directories after moves or deletions. Use a separate policy for repositories shared with Kubo or other writers. The prototype reports candidates and performs no sweep.

**VERIFIED size boundary.** INT07-26 authoritative source and evidence cap and INT10 reconstructed-output ceiling are both 1,048,576 bytes. INT12 checks the declared source length against both limits before promotion, then applies the exact authorized length to readback and checks its digest. At 1 MiB the authentic patched output completed. At 1 MiB plus one, declaration preflight failed without promoting a block, and understating the declaration also failed to produce a receipt. The INT11 producer itself can stage larger files. A live caller must enforce the input limit before the staging producer reads the source. The original producer's truncated-prefix graph also failed the full authoritative digest and yielded no receipt.

## Reproduce

From the repository root with Rust 1.95.0 and a C compiler:

```sh
pin=8cb627b41bd53a555557d7e46a5c1c1070d15dde
agent_dir=integrations/zos-int07-core/agent2
probe="$agent_dir/int11/probe/Cargo.toml"
test_manifest="$agent_dir/int12/Cargo.toml"
scratch=$(mktemp -d)
mkdir "$scratch/patched"
git archive "$pin" Cargo.toml src/ipfs.rs | tar -x -C "$scratch/patched"
git -C "$scratch/patched" apply "$PWD/$agent_dir/int11/INT11_FAIL_CLOSED_PRODUCER.patch"
cargo build --locked --manifest-path "$probe" --target-dir "$scratch/original"
INT11_PASTEBIN_SOURCE="$scratch/patched/src/ipfs.rs" cargo build --locked \
  --manifest-path "$probe" --features patched-writer --target-dir "$scratch/patched-target"
cargo test --locked --manifest-path "$test_manifest"
INT12_ORIGINAL_PROBE="$scratch/original/debug/pastebin-writer-probe" \
INT12_PROBE="$scratch/patched-target/debug/pastebin-writer-probe" \
  cargo test --locked --manifest-path "$test_manifest" -- --ignored --skip tests::crash_child --nocapture
(cd "$agent_dir/int12" && sha256sum -c SHA256SUMS)
```

**VERIFIED.** Six substantive tests passed. The five producer-backed tests ran explicitly, with the internal worker invoked seven times only by its crash parent. Formatting, builds, Clippy and docs passed. The complete commands and exit statuses are in the [Cargo transcript](evidence/validation.log). [Syscall run and counts](evidence/syscall-validation.log) · [SHA-256 manifest](SHA256SUMS)
