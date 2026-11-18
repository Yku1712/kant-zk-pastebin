# ZOS-INT10 — Verified IPFS and UnixFS readback

**NOT PRODUCTION-READY.** Independently verified local readback for the supported UnixFS file subset. Live Pastebin and IPFS publication remain blocked.

## Pinned source audit

The implementation and review branch start at `dc748bb007e458bb88fb4b24647e40809b188d5c`, the frozen head of [Draft PR #6](https://github.com/Yku1712/kant-zk-pastebin/pull/6).

- **Write path.** Pastebin's `pub fn ipfs_add_bytes(data: &[u8]) -> Option<String>` passes bytes through `rust_unixfs::file::adder::FileAdder::default()`, attempts to persist each emitted block through `fn write_block(cid: &impl Display, block: &[u8])`, and returns the last emitted CID string. The source describes balanced DAG-PB files and CIDv0 roots. The writer suppresses directory creation errors, returns unit on block failures, and checks consumption only after the first push. The returned root therefore cannot demonstrate that every block exists or that the complete input was consumed. [Writer and importer](https://github.com/Yku1712/kant-zk-pastebin/blob/dc748bb007e458bb88fb4b24647e40809b188d5c/src/ipfs.rs#L52)
- **On-disk layout.** The reader and writer both derive `KEY = BASE32_NOPAD_UPPER(multihash bytes)`. SHA2-256 keys encode `0x12 0x20` followed by 32 digest bytes. For `n = len(KEY)`, the shard is `KEY[n-3..n-1]`. Stored content is literally the encoded block in `<repo>/blocks/<shard>/<KEY>.data`. The codec and CID version are carried by the CID, while the flatfs key contains only the multihash. Pastebin chooses `IPFS_PATH` or the documented default path. The verifier takes an explicit repository path. [Flatfs construction and lookup](https://github.com/Yku1712/kant-zk-pastebin/blob/dc748bb007e458bb88fb4b24647e40809b188d5c/src/ipfs.rs#L64)
- **Read path.** `pub fn ipfs_cat(cid_str: &str) -> Option<Vec<u8>>` parses the CID, locates the same multihash path, and uses `fs::read` on one block. `pub async fn ipfs_proxy(path: web::Path<String>) -> Result<HttpResponse>` returns that block with inferred content type. Complete file reconstruction, per-block digest checking and authoritative original-byte comparison belong to the verifier delivered here. [Local single-block reader](https://github.com/Yku1712/kant-zk-pastebin/blob/dc748bb007e458bb88fb4b24647e40809b188d5c/src/ipfs.rs#L124) · [HTTP handler](https://github.com/Yku1712/kant-zk-pastebin/blob/dc748bb007e458bb88fb4b24647e40809b188d5c/src/handlers.rs#L3832)
- **Dependency boundary.** The Pastebin lock pins `rust-unixfs 0.5.1` and `ipld-dagpb 0.2.2` to the private `nora` registry, along with `quick-protobuf` and `erdfa-publish`. Service-build and producer compatibility remain blocked on authenticated availability of those dependencies. The standalone crate uses public `cid`, `data-encoding`, `sha2`, and Unix `rustix`; its strict file decoder is independent and validates actual Protobuf bytes. [Pastebin dependency declarations](https://github.com/Yku1712/kant-zk-pastebin/blob/dc748bb007e458bb88fb4b24647e40809b188d5c/Cargo.toml#L20) · [Standalone lockfile](Cargo.lock)

## Verified behavior

| Evidence area | Status | What was exercised |
| --- | --- | --- |
| Raw CID readback | **VERIFIED locally** | Kubo-generated CIDv1 raw block with SHA2-256. Verified encoded bytes and reconstructed authoritative SHA-256. |
| Complete UnixFS DAG readback | **VERIFIED locally for the supported subset** | Offline Kubo generated a single-block CIDv0 file, an empty CIDv0 file, a multiblock CIDv0 graph, a CIDv1 graph with raw leaves, and a nested graph with repeated links. Every fixture matched Kubo's offline `cat` and the original bytes. |
| Synthetic fixture tests | **VERIFIED locally** | Tests construct actual DAG-PB/UnixFS protobuf messages to exercise reordered links, declared sizes, invalid fields, huge lengths, named links, and unsupported types. Fault tests modify or omit Kubo block files in disposable repositories. The cycle guard uses an injected active-path state because a fixture with a valid SHA-256 self-reference would require a fixed point. |
| INT07-26 receipt evidence | **VERIFIED locally** | The optional `int07` adapter uses the real `Publisher`, `ReceiptEvidence` and SQLite journal. Publication authority, receipt provenance and independent spool evidence are fixture-backed. UnixFS retrieval and validation use the actual Kubo-generated blocks. Missing root or child leaves the operation UNRESOLVED. |
| Real Pastebin compatibility | **BLOCKED for runtime validation** | Source-verified layout agreement. The running Pastebin producer and proxy were not exercised with the pinned private dependencies. |
| Live network retrieval, availability or publication | **NOT TESTED** | Fixture generation and comparison used offline Kubo. Live deployment and publication remain blocked. |

## Supported formats and limits

The verifier supports SHA2-256 with exactly 32 digest bytes, CIDv1 raw blocks (`0x55`), and CIDv0 or v1 DAG-PB blocks (`0x70`) containing UnixFS File or legacy Raw file nodes. DAG-PB File nodes may link to raw blocks or supported DAG-PB file nodes. It accepts the two historical DAG-PB node field orders and requires monotonic PBLink fields. Unknown fields and codecs, directory/HAMT/symlink/metadata node types, and optional UnixFS mode or mtime fields are outside this verifier's scope. Missing filesize is interpreted as zero and checked against reconstructed size. Empty or absent link names are accepted for file chunks. [DAG-PB specification](https://ipld.io/specs/codecs/dag-pb/spec/) · [UnixFS specification](https://specs.ipfs.tech/unixfs/)

Every block is hash-checked against the requested CID before decoding or streaming its own bytes. Inline data precedes links in wire order. The verifier requires one `blocksizes` entry per link and checks each actual child's file length, parent filesize, and optional `Tsize` against actual serialized subtree sizes. It compares the final SHA-256 to the authoritative digest. A successful result requires the complete graph and all checks. The output callback must stage or discard bytes until success because a later child or final digest can fail.

Limits can be lowered from these ceilings.

| Limit | Ceiling |
| --- | ---: |
| One encoded block | 1 MiB |
| Cumulative encoded bytes, counting repeated links | 8 MiB |
| Reconstructed output bytes | 1 MiB |
| Node visits, counting every link occurrence | 4,096 |
| Depth below root | 32 |
| Cooperative deadline | 30 seconds |

The decoder borrows field data, checks lengths before slicing and bounds link and chunk-size storage. It performs no decompression. Compressed application payloads remain opaque bytes. The Unix flatfs reader uses descriptor-relative opens with no symlink following for shard and block entries, rejects nonregular files, and bounds allocations and reads. Deadline checks cover traversal, parsing, chunk reads and return from the storage/output ports. A synchronously stalled filesystem or callback needs its own cancellation or an external process deadline.

The INT07 adapter delegates authenticated operation, receipt and expected-root provenance to the original-byte evidence provider. It requires a claimed CID, then streams independently verified reconstructed bytes into the INT07-26 evidence sink. No service wiring is included in the experimental crate.

## Reproduction and evidence

From this crate directory, use the pinned Rust 1.95.0 toolchain.

```sh
sha256sum -c SHA256SUMS
cargo fmt --all --check
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo build --locked --all-features
cargo test --locked --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --all-features --no-deps
```

The fixture catalog is a text transport for exact block and payload bytes. `fixtures/generate.py` invokes Kubo v0.36.0 in empty disposable repositories with `--offline`. It verifies each payload with Kubo `cat`, inventories the root and recursive references, reads each block through Kubo and requires equality to the derived flatfs file. The binary archive SHA-256 was `e327780ee87b1cdef904af8e22cbe3243892e70078b9b16861f61700ce394dc4`. Regeneration reproduced the committed catalog byte for byte. [Kubo release](https://github.com/ipfs/kubo/releases/tag/v0.36.0) · [Kubo execution log](evidence/kubo-validation.log)

The [Rust transcript](evidence/validation.log) contains complete command output and exit statuses. The [pinned regression transcript](evidence/pinned-baseline.log) records the original crate's manifest checks, core tests and local destination tests.
