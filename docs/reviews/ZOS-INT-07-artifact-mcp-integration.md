# ZOS-INT-07 — Artifact MCP ↔ Kant ZK Pastebin experimental review

**Status:** ACCEPT FOR EXPERIMENTAL REVIEW — **NOT PRODUCTION-READY**  
**Date:** 2026-11-18  
**Scope:** Public open-source code and synthetic fixtures only. This is a **review document**, not the full test archive.

## Frozen source identities

- Artifact MCP (public fork): `meta-introspector/artifact-mcp` at `731d5b6e3436cc4bdee961f8510eda6ea2a0d9eb`.
- Kant ZK Pastebin: `13d92d97b9b89f9af94a35cb70436035bd2fd05a`.
- Original source trees were read-only throughout the reported experiments.

## What was tested

| Audit | Result | Scope |
| --- | --- | --- |
| 07-05 | 14/14 PASS | Synthetic Python publication contract |
| 07-06 | 16/16 PASS | Rust/Python differential checks |
| 07-13 | 8/8 PASS | Two synthetic loopback HTTP servers and Rust verifier |
| 07-14 | 6/6 PASS | Synthetic HTTP idempotency, lost ACK, concurrency |
| 07-15 | 7/7 PASS | SQLite transactions, multiprocess retries, SIGKILL before/after commit |
| 07-16 | 7/7 PASS | Synthetic durable loopback HTTP + SQLite |
| 07-17 | 7/7 PASS | Two independent SQLite databases; external publication reconciliation after a bridge crash |
| 07-18 | PASS | Seven stages bundled, source identities and SHA-256 evidence verified |
| 07-19 | PASS | 22/22 archive entries allowlisted, zero findings from scoped pre-publication checks |

The HTTP publishing endpoints, `Idempotency-Key` behavior, and `/operations/{key}` lookup used in the experiments were **synthetic proposed contracts**. The actual Artifact MCP / Pastebin server pair was **not** built or run together. The tests do not establish production-safe exactly-once delivery to Pastebin or IPFS.

In 07-17, all eight concurrent workers returned `Confirmed`, while each SQLite database had one final row. The experiment did **not** instrument the exact number of external POST attempts. This must not be misreported as eight distinct commits or as proof of one network POST.

## Integration findings

1. Artifact publication identity, Pastebin's local pseudo-CID (`bafk` plus a truncated hash), the full witness digest, and a real IPFS CID are distinct concepts. The local pseudo-CID must not be represented as a verified IPFS multihash.
2. Pastebin's public spool and indexing pathway must not be used to publish confidential Senate plaintext. Webhook URL encryption in Artifact MCP is not wallet encryption.
3. A lost or poisoned HTTP response may follow a successful external side effect. Local SQLite durability alone cannot provide atomicity with a separate service. Reconciliation requires an agreed externally queryable identity and idempotent publication behavior.
4. The `GET /ipfs/{cid}` path has not been shown to reconstruct and verify arbitrary UnixFS multiblock content. Do not assume a returned block proves artifact-byte equality.
5. Actual compilation remains blocked by dependency resolution: Artifact MCP requires `askama 0.16.1` (absent from the inspected offline cache); Pastebin uses four `nora` sparse registry packages at `solana.solfunmeme.com`. This registry is operated on private Mike-controlled infrastructure and must not be bypassed.

### Locked `nora` packages

| Crate | Version | Locked checksum |
| --- | --- | --- |
| erdfa-publish | 0.1.0 | `a3537ebb306c2ad3521e206a938415e42bea47df8fcb56a8862a9a751fa8d44e` |
| ipld-dagpb | 0.2.2 | `0e51fc90b345e57d8d5346dcef5eb16d51d1d19572a8a2d23c15009904e68244` |
| quick-protobuf | 0.8.1 | `9d6da84cc204722a989e01ba2f6e1e276e190f22263d0cb6ce8526fcdb0d2e1f` |
| rust-unixfs | 0.5.1 | `c6d3eadb39df5a598445aca058dd00217c96f7e2311c45c53388164fc42d11fb` |

## Review bundle identity

- Local source archive: `ZOS_INT07_18_REVIEW.tar.gz`
- SHA-256: `c0c22cea8b9f30da38b3f3aee850ec753efe6e896d9701cfb569f0b08b906362`
- 07-19 reported 22 expected files, no allowlist, SHA-256, or specified secret-pattern findings. These checks are scoped; they do not guarantee detection of all sensitive information.
- **IPFS CID: pending publication by the artifact owner.** The archive is not uploaded by this PR, and its contents have not been independently retrieved by this GitHub publication workflow.

## Proposed decisions for maintainer review

1. Approve or revise the mapping between Artifact MCP artifact/revision/tenant and Pastebin publication identity.
2. Decide whether idempotency keys and read-only operation lookup should be added to Pastebin, or owned by a distinct gateway, with clear crash-recovery semantics.
3. Define the boundary between public artifact publication and any future encrypted private Senate or wallet workflows; specify authentic dependency distribution and verification.

## Change scope and safety

This PR adds only this documentation. No production implementation, API claims, repository merges, protected branch changes, database modifications, credentials, or deployment operations. No client-confidential source code is included. Acceptance for production remains **BLOCKED** pending maintainer review, authentic dependency resolution, real protocol integration, authorization review, IPFS byte verification, and crash/fault tests against the real implementations.
