# ZOS-INT14 independent security and recovery audit

**Verdict: NOT PRODUCTION-READY.** Independent reproduction passed. INT13 implements point-in-time authorization checks and durable local recovery. It does not enforce revocation through destination commit, bridge confirmation, or delivery of the result. The test authority is synthetic. The SQLite and UnixFS results are from the real pinned experimental components.

## Independent reproduction and provenance

The audit fetched the two reviewed commits from GitHub into a new repository and exported the INT13 tree, Git tree object `b8371188f9c7aabec8c6bd35fecfbf842f70e279`. It used fresh Cargo target directories and an independently rebuilt producer. The offline execution used cached public dependency sources and the pinned locks. The reproduction script uses a fresh workspace per run and can fetch public dependencies with Cargo's default online setting.

| Component | Source identity and role |
| --- | --- |
| INT13 / PR #10 | `113ec894865a6f16c8d8f9bdfc559c72abd02c82`, harness and original tests |
| INT12 / PR #9 | `361fae9395e4c7a18bbecf0099f2c613cc785d6d`, exact INT13 parent and store |
| INT11 / PR #8 | `8cb627b41bd53a555557d7e46a5c1c1070d15dde`, writer source export and fail-closed patch from INT13 tree, isolated probe |
| INT10 / PR #7 | `ec90d56a74dbd0810fcd4849f42633386a4207f7`, UnixFS verifier inherited through INT13 |
| INT08 / PR #5 | `a9dde4d712dc0c99c3ef2b30d92ef49723695c77`, authority contract and independent legacy fixture export |
| INT07-26 / PR #6 | `dc748bb007e458bb88fb4b24647e40809b188d5c`, inherited publisher, evidence and journal |

| Check | Classification |
| --- | --- |
| INT13 manifest against exported bytes | **PASS**, 15 of 15 |
| Corrected probe rebuilt with new target | **PASS**, compiler and source/binary hashes in transcript |
| Unmodified INT13 tests | **PASS**, 8 of 8 |
| Both SIGKILL boundaries rerun separately | **PASS**, signal 9 observed by parent, databases passed integrity check, one receipt recovered each |
| INT14 supplemental tests | **PASS**, 6 test functions, including 28 authority boundary cases and six stale-sample cases |
| Legacy fixture compilation against current hardened core | **FAIL** as expected, exactly two E0050 diagnostics, exit 101 |
| Legacy fixtures with the test-only patch | **PASS**, 12 of 12 |
| Original INT08 manifest against exact Git bytes | **FAIL**, 13 mismatches, all 13 matched after CRLF conversion |
| Independent INT08 byte-exact manifest | **PASS**, 13 of 13, original evidence preserved |
| Continuous or commit-time authorization | **FAIL** in the isolated fixture model, details below |
| Live INT09 and authenticated Pastebin integration | **BLOCKED**, missing live authority and authenticated application evidence |
| IPFS network publication, power loss, kills inside SQLite COMMIT, malicious process with local write access | **NOT_TESTED** |

[Fresh fetch, compiler and baseline tests](evidence/reproduction.log) · [Supplemental tests](evidence/audit.log) · [Reproducer transcript](evidence/reproducer.log) · [Legacy compile and patched tests](evidence/legacy-tests.log) · [Legacy manifest comparison](evidence/legacy-manifest.log)

## Threat model

The bridge principal, snapshot policy, destination release authority, writer, canonical storage, and two ledgers have distinct roles. An authentic user can lose access after receiving a snapshot. Release permission can expire, be revoked or reissued, or become unobservable. Work can finish after the initiating process dies or loses the acknowledgement. A storage fault can remove or alter a block after confirmation. Concurrent operations can share content-addressed blocks. The test boundaries model these independently using a controllable clock, a separate authority thread, genuine subprocess death and local filesystem faults. The repository paths and the local destination API caller are trusted in this harness. Host compromise, a real authority implementation and remote service identity are outside the tested model.

Three facts must remain independently observable.

1. **Admission** means the source supplied an authorized snapshot and release checks allowed dispatch at their sample times.
2. **Local commit** means the destination's independent SQLite receipt exists and records its completed local graph. Canonical blocks can exist before this commit. The receipt is historical evidence even if a later readback fails.
3. **Current bridge visibility** means an authorized bridge execution returned a currently verified receipt at its observed check points. A persisted `CONFIRMED` row records historical journal state. A low-level destination or journal lookup is an internal API and must stay behind an access guard.

## Authority-boundary matrix

One iteration per policy change and boundary. When the result was denied, revocation and expiry produced `Ineligible`, and failed release lookup produced `ExternalUnavailable`. Concurrent revoke/regrant flipped authorization on a separate thread, asserted an intervening denied state, then regranted the same fixture intent without a generation field.

| Boundary exercised | Revocation | Expiry | Failed lookup | Concurrent revoke/regrant |
| --- | --- | --- | --- | --- |
| Before initial public release check | Denied, no rows | Same | Same | Fresh publication confirmed |
| After an earlier release decision, before INT08 fixture snapshot read | Denied after source read, no rows | Same | Same | Fresh publication confirmed |
| Staging handoff, corrected writer finished and staged DAG exists | Denied, one local receipt, bridge UNRESOLVED | Same | Same | Confirmed |
| Destination accepted, first child promotion before COMMIT | Denied, one local receipt, bridge UNRESOLVED | Same | Same | Confirmed |
| After destination COMMIT, before acknowledgement | Denied, one local receipt, bridge UNRESOLVED | Same | Same | Confirmed |
| Reconciliation after real destination lookup, before bridge evidence verification | Denied, one local receipt, bridge UNRESOLVED | Same | Same | Confirmed by lookup |
| After historical bridge confirmation, before the next bridge request | Denied, immutable CONFIRMED, one local receipt | Same | Same | Historical confirmation returned |

The staging test interrupts at the producer-to-store handoff. Authority changes inside the producer subprocess were **NOT_TESTED**. The pre-COMMIT tests interrupt during promotion, before the destination transaction can commit. The raw source reads an authorized snapshot before the independent public-release check. This means an earlier public-release lookup failure can prevent dispatch while the source read itself still happens under the source policy.

**Stale positive samples.** A policy read sampled true, then the authority changed on a separate thread before that read returned. At the pre-journal-confirm sample, revocation, expiry and lookup loss each left the journal historically CONFIRMED. The subsequent INT13 return check denied it. At the final return sample, all three changes still allowed the invocation to return confirmation. This shows the allowed result is bounded by the sample's linearization point. It also shows that repeated release checks cannot provide a commit-time or return-time guarantee.

**Source-grant revocation.** Separately, the test revoked the actual INT08 fixture's read/release grant after snapshot materialization while destination release permission remained true. The in-flight operation returned confirmation. A second execution failed source authorization. An authentic INT08 provider must honor the INT07 `AuthorizedSource` callback contract through a guard or an immutable authorized release. The tested fixture releases its source lock immediately after snapshot creation. [Supplemental test source](int14_tests.rs)

## Crash and recovery transitions

| Interruption / next action | Bridge | Destination receipt | Filesystem and result |
| --- | --- | --- | --- |
| Admission denied | No row | None | No writer stage or promotion |
| SIGKILL after canonical root promotion, before destination commit | UNRESOLVED | None | Canonical root is inventoried as an orphan. Staging retains source bytes. Lookup alone does not restage. |
| Authorized explicit retry of that operation | CONFIRMED after verification | One | Same operation identity, one verified receipt, orphan root becomes referenced |
| SIGKILL after destination commit, before submit acknowledgement | UNRESOLVED | One | Canonical graph verifies. Staging still exists. |
| Authorized lookup or repeated retry after that commit | CONFIRMED after verification | Same one | No second publication or receipt |
| Release denial/unavailability after destination acceptance | UNRESOLVED, unless confirmation had already passed a stale positive sample | One | No implicit rollback. Current call fails if its policy sample observes denial. |
| After bridge confirmation, then block loss/corruption | Immutable CONFIRMED | Same one | Current verifier rejects lookup; ordinary retry does not repair it. |
| Controlled repair restores the exact missing or corrupt block | Immutable CONFIRMED | Same one | Fresh lookup verifies the repaired bytes. |

Both SQLite files reopened with `integrity_check = ok`. The tests observed actual SIGKILL statuses and checked canonical root bytes and staging files. These process-crash results on OverlayFS do not measure power-loss durability. INT07 and INT12 transactions are independent. [Execution and filesystem observations](evidence/audit.log)

## Historical state and current integrity

The test verified valid storage, missing root, missing child, corrupt child, a valid destination receipt with an UNRESOLVED bridge journal, synchronous lookup/retry after damage, and a reader racing a single-block atomic repair. Missing root/child returned `ExternalUnavailable`. A corrupted child returned `InvalidReceipt`. Repeated lookup and explicit retry left the damaged path and bridge receipt unchanged. In the fixture, authorized repair restored the byte-exact block using a synchronized temporary file, same-directory rename and parent sync. Concurrent readers observed only a typed failure or the identical confirmed receipt. This does not establish a multi-block repair transaction.

A public API should authorize the caller before revealing either dimension. Internally, represent the dimensions separately. For example:

```rust
enum CurrentAvailability {
    VerifiedAt { receipt: ConfirmedReceipt, checked_at: SystemTime },
    MissingBlock,
    Corrupt,
    Unavailable,
    Unchecked,
}
struct PublicationObservation {
    history: OperationState,
    current: CurrentAvailability,
}
```

Bind availability to the observed receipt and verification time. Keep historical receipt rows immutable. Repair approval, source-of-truth validation, and later verification belong in separate audit events. A raw historical state or destination receipt should not be used as an availability result or an authorization token.

## Findings and proposed remediation

| Severity | Finding | Proposed remediation |
| --- | --- | --- |
| **High** | An accepted operation can physically commit after the release is revoked, expires or becomes unavailable. The destination has no release generation or commit-time guard. | Define the external authorization linearization point. If revocation must prevent a subsequent commit, require destination enforcement of a signed, audience- and operation-bound capability plus current generation or a shared reservation/commit fence. Expiry alone bounds exposure and cannot synchronously revoke prior grants. |
| **High** | Source snapshot permission and the final release sample can become stale. A sample immediately before journal confirmation can also be stale before the SQLite write. Same-intent revoke/regrant has no generation fence. | Hold the source guard across the callback or use an immutable authorized release. Define whether regrant can reconcile an earlier effect and record which generation authorized admission and acceptance. Separate policy-denied recovery from provider absence so a privileged reconciler can account for effects without republishing. |
| **Medium** | Crash-retained staging contains plaintext. The canonical orphan inventory intentionally excludes the sibling staging area. | Give staging private access and bounded retention. Coordinate quarantine with worker and writer ownership. A pre-probe crash may leave a producer child alive, so stage age or worker PID alone is insufficient. |
| **Medium** | Historical confirmation can be surfaced without live availability if a caller bypasses the INT13 wrapper. Ordinary retries do not repair damage. | Return typed current verification separately and gate raw journal APIs. Restore exact blocks through an authorized, audited repair workflow. |
| **Low** | Legacy fixtures use outdated observer signatures, and INT08 evidence hashes record CRLF bytes instead of Git blobs. | Apply the isolated fixture-only adapter when testing against the hardened core. Use the independent byte-exact manifest alongside the original. |

No production policy or storage code is changed by the review. An enforceable cross-service protocol needs the real authority contracts and provider participation. The [experimental compatibility patch](INT08_TEST_FIXTURES.patch) modifies only legacy fixtures and is applied in a separate export. The core, original fixture blobs and all historical manifests remain available unchanged.

## Storage lifecycle and cleanup boundary

Two successful revisions shared one canonical block key. While the second writer held the destination transaction, a concurrent reader verified the first receipt and saw no second receipt. Inventory used the writer lock and finished after the second transaction, with no orphans. The SIGKILL fixtures quarantined the dead worker's staging directory by rename only after joining the sole process at a post-probe hook. Canonical blocks stayed in place. Duplicate retries returned one receipt. This verifies non-destructive quarantine in an isolated fixture, not a production stage collector.

Canonical inventory is a snapshot under the destination writer lock. Using that snapshot to delete after the lock releases can race a later adoption. A future collector needs reachability across every committed receipt and reserved graph, exclusive writer/repair coordination through the deletion decision, and reader leases or a quiescence protocol. It needs to distinguish old staging owned by a dead worker from a producer that is still running. No automated destructive collection is included.

## Reproduction artifacts

Run `./reproduce.sh` from a checkout of the review using Linux, Rust/Cargo 1.95.0, a C compiler, Python 3 and ripgrep. The script reports its fresh evidence directory and writes `transcript.log`. Set `CARGO_NET_OFFLINE=true` if the public Cargo dependencies are already available locally. It runs the original tests before adding the supplemental module to the disposable export. It applies the legacy test-only patch in a separate export and asserts the expected unpatched failures first. [Reproducer](reproduce.sh) · [Byte-exact INT08 manifest](INT08_GIT_SHA256SUMS) · [INT14 manifest](SHA256SUMS)
