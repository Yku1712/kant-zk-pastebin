# INT07-26 — Independent security hardening

**NOT PRODUCTION-READY.** Reviewed against PR #4 at `3274a25809f1cd109b62ffa553c94a198adade80`. The separate review contains proposed changes to the standalone core and the experimental local SQLite destination.

## Findings on the pinned candidate

| Finding | Outcome | Evidence |
| --- | --- | --- |
| SQLite restore used weaker CID validation than the live path. | **FAIL, reproduced.** A syntactically valid but unsupported codec survived restore. | Pinned `journal::read_state` constructed `VerifiedIpfs` after parsing alone. The defensive test expected `JournalConflict`. [Pinned source](https://github.com/Yku1712/kant-zk-pastebin/blob/3274a25809f1cd109b62ffa553c94a198adade80/integrations/zos-int07-core/src/journal.rs#L174) |
| Revocation could occur between the initial policy check and confirmation. | **FAIL, reproduced.** With a fixture holding the dispatch response, revoking eligibility did not produce the required `Ineligible` result on the pin. | The pinned coordinator called `permits` before dispatch and confirmed after readback without a new policy decision. [Pinned coordinator](https://github.com/Yku1712/kant-zk-pastebin/blob/3274a25809f1cd109b62ffa553c94a198adade80/integrations/zos-int07-core/src/lib.rs#L320) |
| Resource limits were delegated. | **Source-verified gap.** Whole readback vectors were returned to the core. The local destination already imposed a 1 MiB input cap and SQL length predicate. | [Pinned evidence contract](https://github.com/Yku1712/kant-zk-pastebin/blob/3274a25809f1cd109b62ffa553c94a198adade80/integrations/zos-int07-core/src/lib.rs#L263) |
| Retry used an ordinary destination hook. | **Source-verified gap.** Its default was a no-op, but the override shared the base trait with ordinary destinations. | [Pinned destination contract](https://github.com/Yku1712/kant-zk-pastebin/blob/3274a25809f1cd109b62ffa553c94a198adade80/integrations/zos-int07-core/src/lib.rs#L243) |

The baseline's original 23 core tests and 3 local-destination tests passed. Independently hashing the pinned blobs with `git cat-file` matched all 35 manifest entries. [Baseline transcript](agent2/int07_26/baseline.log) · [Git-byte evidence](agent2/int07_26/pin-git-bytes.log) · [Two expected baseline failures](agent2/int07_26/pin-regressions.log)

## Proposed remediations and verified behavior

- **Receipt restore.** Apply the same canonical CID, supported codec, SHA-256 algorithm, multihash length, and raw payload digest checks on restore and live confirmation. The restore path rejects malformed text, overlong strings, URI-wrapped input, unknown codec or hash, short multihashes, and an unrelated raw digest. Cap persisted remote ID and CID text before copying into Rust. Fixture tests also check malformed receipt IDs, schema-version mismatch, and SQLite constraints on incompatible states. The local destination rejects corrupted persisted body bytes before emitting them. An attacker with write access to a journal can still replace well-formed record data. Restrict journal access to the trusted process and reconcile damaged data independently.
- **Bounded evidence.** Cap source payloads at 1 MiB before hashing and journaling. Stream each readback into a fixed-memory SHA-256 sink with a 1 MiB byte budget. An overflow remains fatal even when a provider ignores the append error. The local provider applies the smaller of its own cap and the sink's remaining budget in SQL before making its temporary byte allocation. The evidence interface now requires a streaming output argument and existing fixtures have migrated. Source and transport adapters still own bounds before allocating incoming data. The synthetic graph test imposes encoded bytes, node visits, maximum depth, and deadline limits and rejects a missing child. The fixture graph uses explicit linked fragments and a separate traversal budget.
- **Release consistency.** Check current eligibility again immediately before the external submit or retry and after receipt verification. Recheck before returning a prior confirmation. The interval between a policy check and the next action still requires a release guard or immutable release authorization that spans the authorized source callback. A deterministic local lock fixture verifies serialization through confirmation. In the two-database test, the provider committed first and eligibility was then revoked. The bridge returned `Ineligible` and left the operation UNRESOLVED while the provider's one committed record remained. Live integration must define when revocation takes effect relative to provider acceptance. A bridge check cannot cancel a submission already accepted by the provider.
- **Durable retry.** Require a separate `DurableRetry` implementation and explicit capability opt-in from the same destination instance, in addition to the caller selecting retry. With no capability, an inconclusive lookup stays UNRESOLVED. A capability from a different object with the same destination label is rejected before retry. The real local SQLite regression verifies that overlapping first submission and retry produce one provider record. A Rust adapter's capability remains a trusted assertion about service behavior. A live provider needs independently established durability before exposing it.

**PASS for the proposed patch.** Applied the [patch](agent2/int07_26/INT07_26_PROPOSED.patch) to an exact Git export and ran the locked offline builds, formatting, Clippy, and docs for both crates. The core passed 31 tests and the experimental destination passed 5, with both original suites preserved. Two ignored child entry points are invoked by their tests. [Hardened execution transcript](agent2/int07_26/hardened.log)

**BLOCKED for full UnixFS and live integration.** The linked-fragment tests use synthetic evidence. Authentic DAG-PB/UnixFS decoding, linked-block hash verification, complete byte reconstruction, and the live source/provider authorization and durable retry contracts remain unverified.

## Reproduction

From the repository root, apply the proposed patch to a clean checkout of the pinned commit. Run the following from `integrations/zos-int07-core/` using the pinned Rust toolchain.

```sh
cargo test --locked --test adversarial_security_suite pinned_regressions::
cargo test --locked --test adversarial_security_suite hardening::
cargo test --locked
cargo test --locked --manifest-path agent2/experimental/Cargo.toml
```

To reproduce the two baseline failures, use a separate exact export of the pin and add only [pinned_regressions.rs](tests/int07_26/pinned_regressions.rs) at `tests/int07_26/pinned_regressions.rs`. Append these declarations to the original `tests/adversarial_security_suite.rs`.

```rust
#[path = "int07_26/pinned_regressions.rs"]
mod pinned_regressions;
```

Run `cargo test --locked --test adversarial_security_suite pinned_regressions::`. The expected baseline exit is 101 with two assertion failures. The patched tree passes both. The SQLite mutation stays within disposable databases and testing uses fixture destinations.
