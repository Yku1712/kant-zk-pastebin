# ZOS-INT11 — Pastebin producer security audit

**NOT PRODUCTION-READY.** The original writer is compatible with INT10 for complete, readable DAGs in the tested two-chunk range. Runtime testing identified input truncation and false storage acknowledgements outside those conditions.

## Pinned source and dependency identity

| Evidence | Identity |
| --- | --- |
| Pastebin and INT10 commit | `ec90d56a74dbd0810fcd4849f42633386a4207f7` |
| Original `src/ipfs.rs` SHA-256 | `fa26cf8b2ccdd2aeabe3324efc1ccacd8d13becf660d7b743eb62e16182c9b3e` |
| Original Pastebin `Cargo.lock` SHA-256 | `ea2b48d73f498eb0d47d78c77519c6105ea02e93238b7dd698ae3693b40bdf1a` |
| `rust-unixfs 0.5.1` archive SHA-256 | `c6d3eadb39df5a598445aca058dd00217c96f7e2311c45c53388164fc42d11fb` |
| `rust-unixfs` publisher VCS identity | `eae2225b89bddf6518d1647387a5695e5d0da1f0`, path `unixfs` |
| `ipld-dagpb 0.2.2` archive SHA-256 | `0e51fc90b345e57d8d5346dcef5eb16d51d1d19572a8a2d23c15009904e68244` |
| `quick-protobuf 0.8.1` archive SHA-256 | `9d6da84cc204722a989e01ba2f6e1e276e190f22263d0cb6ce8526fcdb0d2e1f` |

The isolated lock uses the independent public crates.io source. The provenance check compared actual public archive bytes against the checked-in lock checksums for the writer, codec and hash dependency set. Sixteen package/version archives matched. The same check records twelve differing supporting package versions in the isolated probe closure. The probe build script includes the original repository bytes from `fn ipfs_repo` through `ipfs_cat`. It adds only the imports and CLI needed to exercise those functions. These tests execute the real function bodies and encoder without registry remapping or any private registry request. [Original writer](https://github.com/Yku1712/kant-zk-pastebin/blob/ec90d56a74dbd0810fcd4849f42633386a4207f7/src/ipfs.rs#L52) · [Archive identity transcript](evidence/package-provenance.log) · [Probe lock](probe/Cargo.lock)

## Source-reviewed format contract

- `FileAdder::push(&mut self, input: &[u8]) -> (impl Iterator<Item = (Cid, Vec<u8>)>, usize)` returns blocks plus consumed input bytes. Its fixed-size default is 262,144 bytes. Each call consumes at most the space remaining in a single chunk. `finish(self)` emits the remaining leaf and link blocks from the bytes it has received. The default balanced collector has fanout 174 and accumulates parent nodes from file byte lengths and encoded subtree totals. [Exact public FileAdder source](https://github.com/dariusc93/rust-ipfs/blob/eae2225b89bddf6518d1647387a5695e5d0da1f0/unixfs/src/file/adder.rs#L154)
- Default leaves have UnixFS `Type=File`, optional inline bytes, and explicit filesize. Empty files omit inline bytes and encode filesize 0. Parents have ordered links, explicit empty link names, per-child file sizes in `blocksizes`, cumulative serialized subtree sizes in `Tsize`, and total logical filesize. The encoder writes DAG-PB links before Data and hashes the entire serialized block with SHA2-256, emitting CIDv0 for every block. This shape fits INT10's supported DAG-PB subset. [Leaf, codec and collector](https://github.com/dariusc93/rust-ipfs/blob/eae2225b89bddf6518d1647387a5695e5d0da1f0/unixfs/src/file/adder.rs#L228) · [DAG-PB serialization](https://github.com/dariusc93/rust-ipfs/blob/eae2225b89bddf6518d1647387a5695e5d0da1f0/unixfs/src/pb.rs#L81) · [INT10 format audit](../unixfs/INT10_VERIFIED_READBACK.md)
- Pastebin derives uppercase unpadded base32 of the multihash bytes as the flatfs key. It writes the literal serialized block to `<repo>/blocks/<key[n-3..n-1]>/<key>.data`. `ipfs_cat` uses the same path and returns a single encoded block. The IPFS HTTP proxy calls that one-block reader. INT10 rehashes every requested block, reconstructs links in order and compares the complete result against the authoritative SHA-256. [Pinned storage and reader](https://github.com/Yku1712/kant-zk-pastebin/blob/ec90d56a74dbd0810fcd4849f42633386a4207f7/src/ipfs.rs#L59) · [Pinned handler](https://github.com/Yku1712/kant-zk-pastebin/blob/ec90d56a74dbd0810fcd4849f42633386a4207f7/src/handlers.rs#L3832)

## Findings

| Finding | Scope and verified outcome |
| --- | --- |
| Unconsumed input is silently omitted. | **VERIFIED.** The original pushes at most twice, discards the second consumed count, then finalizes. Inputs with a common two-chunk prefix produced the same real CID even when the authoritative input differed. INT10 rejected those inputs by digest. |
| Missing or failed storage can still yield a CID. | **VERIFIED.** The block helper returns unit and caller advances the root on every emitted block. Isolated missing, blocked and partially written repositories reproduced false successful root reporting. |
| Direct writes can expose a partial block. | **SOURCE-REVIEWED.** The original opens its final filename through `fs::write`. A write error or an overlapping read can observe a truncated file. Isolated tests removed or shortened an actual child and verified INT10 rejection. Concurrent reader and power-loss tests are **NOT TESTED**. |
| The root identifies local bytes. | **SOURCE-REVIEWED.** Pinning, exchange, remote availability and an authenticated publication receipt require separate contracts. A complete local DAG alone does not establish them. |
| Caller may still report spool success. | **SOURCE-REVIEWED.** Paste and upload handlers can preserve successful local paste storage with an absent optional `ipfs_cid`. The patch keeps that API behavior and changes the root report itself. [Upload handler](https://github.com/Yku1712/kant-zk-pastebin/blob/ec90d56a74dbd0810fcd4849f42633386a4207f7/src/upload_handler.rs#L178) |

## Proposed remediation

The [experimental patch](INT11_FAIL_CLOSED_PRODUCER.patch) consumes input until empty and rejects no progress or overconsumption. It resolves the repository once, treats write errors as failures, and persists each completed block through a sibling temporary file and rename. It moves the already-locked `tempfile` dependency to normal dependencies. An earlier completed block may remain after failure. The writer withholds the root on failure. Graph-wide rollback and power-loss durability remain outside this patch. The isolated probe built and tested the exact source obtained by applying the patch to a clean pinned export. Production routes and build manifests in the review branch are unchanged.

## Remaining blockers

- **Locked full-service build: BLOCKED.** The original dependency source still resolves four direct packages through `nora`: `rust-unixfs 0.5.1`, `ipld-dagpb 0.2.2`, `quick-protobuf 0.8.1` and `erdfa-publish 0.1.0`. An authenticated resolver for the entire original graph was unavailable within scope. The recorded offline full-service check stopped first at uncached public `actix-files` with exit 101, so it did not exercise private dependency resolution. [Locked service check](evidence/service-blocker.log) · [Pinned dependency declarations](https://github.com/Yku1712/kant-zk-pastebin/blob/ec90d56a74dbd0810fcd4849f42633386a4207f7/Cargo.toml#L20)
- **Live service compatibility: BLOCKED.** Actual deployment configuration, concurrent storage behavior, and an authenticated receipt and authorization path were outside isolated producer execution.
- **Live IPFS network and durability: NOT TESTED.** Kubo validation used offline repositories. No claim covers network publication or crash durability.

[Compatibility matrix and commands](INT11_COMPATIBILITY_MATRIX.md) · [Complete validation transcript](evidence/complete-validation.log)
