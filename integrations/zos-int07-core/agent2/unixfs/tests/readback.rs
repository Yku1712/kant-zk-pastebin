// SPDX-License-Identifier: Apache-2.0
#![cfg(unix)]

use cid::multihash::Multihash;
use data_encoding::{BASE32_NOPAD, HEXLOWER};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use zos_unixfs_readback::{BlockSource, Cid, Error, Flatfs, Limits, verify};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("zos-unixfs-{}-{suffix}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create disposable directory: {error}"),
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn decode(hex: &str) -> Vec<u8> {
    if hex == "-" {
        return Vec::new();
    }
    HEXLOWER.decode(hex.as_bytes()).unwrap()
}

struct Fixture<'a> {
    name: &'a str,
    root: Cid,
    payload: Vec<u8>,
    blocks: Vec<(&'a str, Vec<u8>)>,
}

fn fixtures() -> Vec<Fixture<'static>> {
    let mut fixtures = Vec::new();
    for line in include_str!("../fixtures/kubo-v0.36.0.txt").lines() {
        let parts: Vec<_> = line.split_ascii_whitespace().collect();
        match parts.as_slice() {
            ["case", name, cid, bytes] => fixtures.push(Fixture {
                name,
                root: cid.parse().unwrap(),
                payload: decode(bytes),
                blocks: Vec::new(),
            }),
            ["block", path, bytes] => fixtures
                .last_mut()
                .unwrap()
                .blocks
                .push((path, decode(bytes))),
            _ => panic!("invalid committed fixture line"),
        }
    }
    fixtures
}

fn materialize(fixture: &Fixture<'_>, root: &Path) {
    for (path, bytes) in &fixture.blocks {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

fn cid_path(root: &Path, cid: &Cid) -> PathBuf {
    let key = BASE32_NOPAD.encode(&cid.hash().to_bytes());
    root.join("blocks")
        .join(&key[key.len() - 3..key.len() - 1])
        .join(format!("{key}.data"))
}

#[test]
fn kubo_offline_raw_v0_v1_and_nested_dags_reconstruct_exactly() {
    let fixtures = fixtures();
    assert_eq!(fixtures.len(), 6);
    for fixture in fixtures {
        let temp = Scratch::new();
        materialize(&fixture, &temp.0);
        let store = Flatfs::open(&temp.0).unwrap();
        let mut output = Vec::new();
        let stats = verify(
            &store,
            &fixture.root,
            digest(&fixture.payload),
            Limits::default(),
            |chunk| {
                output.extend_from_slice(chunk);
                Ok(())
            },
        )
        .unwrap_or_else(|error| panic!("{}: {error}", fixture.name));
        assert_eq!(output, fixture.payload, "{}", fixture.name);
        assert_eq!(stats.bytes, fixture.payload.len());
        assert_eq!(stats.sha256, digest(&fixture.payload));
        if fixture.name.starts_with("multiblock") {
            assert!(stats.nodes > 1);
        }
        if fixture.name == "nested-v1" {
            assert!(
                stats.nodes > fixture.blocks.len(),
                "shared links must be counted per visit"
            );
            let limits = Limits {
                depth: 1,
                ..Limits::default()
            };
            assert!(matches!(
                verify(
                    &store,
                    &fixture.root,
                    digest(&fixture.payload),
                    limits,
                    |_| Ok(())
                ),
                Err(Error::Limit("depth"))
            ));
        }
    }
}

#[test]
fn flatfs_missing_altered_truncated_and_symlinked_blocks_fail_closed() {
    let fixture = fixtures()
        .into_iter()
        .find(|f| f.name == "multiblock-v1")
        .unwrap();
    let root_temp = Scratch::new();
    materialize(&fixture, &root_temp.0);
    let store = Flatfs::open(&root_temp.0).unwrap();
    let root = cid_path(&root_temp.0, &fixture.root);
    let child = fixture
        .blocks
        .iter()
        .map(|(path, _)| root_temp.0.join(path))
        .find(|path| *path != root)
        .unwrap();
    let bytes = fs::read(&child).unwrap();
    let expected = digest(&fixture.payload);
    let read = || {
        verify(&store, &fixture.root, expected, Limits::default(), |_| {
            Ok(())
        })
    };

    fs::remove_file(&child).unwrap();
    assert!(matches!(read(), Err(Error::MissingBlock(_))));
    fs::write(&child, &bytes[..bytes.len() - 1]).unwrap();
    assert!(matches!(read(), Err(Error::HashMismatch(_))));
    let mut altered = bytes.clone();
    altered[0] ^= 1;
    fs::write(&child, altered).unwrap();
    assert!(matches!(read(), Err(Error::HashMismatch(_))));

    fs::remove_file(&child).unwrap();
    let elsewhere = root_temp.0.join("untrusted-target");
    fs::write(&elsewhere, bytes).unwrap();
    std::os::unix::fs::symlink(elsewhere, child).unwrap();
    assert!(matches!(read(), Err(Error::Io(_))));
}

#[derive(Default)]
struct Memory(HashMap<Cid, Vec<u8>>);

impl Memory {
    fn put(&mut self, codec: u64, bytes: Vec<u8>) -> Cid {
        let cid = Cid::new_v1(codec, Multihash::wrap(0x12, &digest(&bytes)).unwrap());
        self.0.insert(cid, bytes);
        cid
    }
}

impl BlockSource for Memory {
    fn read_block(&self, cid: &Cid, limit: usize, _: Instant) -> Result<Vec<u8>, Error> {
        let block = self.0.get(cid).ok_or(Error::MissingBlock(*cid))?;
        if block.len() > limit {
            return Err(Error::Limit("block or encoded bytes"));
        }
        Ok(block.clone())
    }
}

fn varint(output: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        output.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    output.push(value as u8);
}

fn number(output: &mut Vec<u8>, tag: u64, value: u64) {
    varint(output, tag << 3);
    varint(output, value);
}

fn bytes(output: &mut Vec<u8>, tag: u64, value: &[u8]) {
    varint(output, (tag << 3) | 2);
    varint(output, value.len() as u64);
    output.extend_from_slice(value);
}

fn unixfs(kind: u64, inline: &[u8], size: u64, children: &[u64]) -> Vec<u8> {
    let mut message = Vec::new();
    number(&mut message, 1, kind);
    bytes(&mut message, 2, inline);
    number(&mut message, 3, size);
    let mut packed = Vec::new();
    for child in children {
        varint(&mut packed, *child);
    }
    bytes(&mut message, 4, &packed);
    message
}

fn dag(data: &[u8], links: &[(Cid, &str, u64)]) -> Vec<u8> {
    let mut message = Vec::new();
    for (cid, name, size) in links {
        let mut link = Vec::new();
        bytes(&mut link, 1, &cid.to_bytes());
        bytes(&mut link, 2, name.as_bytes());
        number(&mut link, 3, *size);
        bytes(&mut message, 2, &link);
    }
    bytes(&mut message, 1, data);
    message
}

#[test]
fn authentic_wire_inline_data_packed_sizes_and_link_order() {
    let mut store = Memory::default();
    let first = store.put(0x55, b"AA".to_vec());
    let second = store.put(0x55, b"BB".to_vec());
    let file = unixfs(2, b"start", 9, &[2, 2]);
    let good = store.put(0x70, dag(&file, &[(first, "", 2), (second, "", 2)]));
    let reversed = store.put(0x70, dag(&file, &[(second, "", 2), (first, "", 2)]));
    let mut output = Vec::new();
    verify(
        &store,
        &good,
        digest(b"startAABB"),
        Limits::default(),
        |chunk| {
            output.extend_from_slice(chunk);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(output, b"startAABB");
    assert!(matches!(
        verify(
            &store,
            &reversed,
            digest(b"startAABB"),
            Limits::default(),
            |_| Ok(())
        ),
        Err(Error::DigestMismatch)
    ));
}

#[test]
fn protobuf_link_and_size_failures_are_distinct() {
    let mut store = Memory::default();
    let child = store.put(0x55, b"one".to_vec());
    let file = unixfs(2, b"", 3, &[3]);
    let invalid_link = store.put(0x70, dag(&file, &[(child, "named", 3)]));
    let missing_link_size = store.put(0x70, dag(&unixfs(2, b"", 0, &[]), &[(child, "", 3)]));
    let wrong_total = store.put(0x70, dag(&file, &[(child, "", 2)]));
    let wrong_child = store.put(0x70, dag(&unixfs(2, b"", 2, &[2]), &[(child, "", 3)]));
    let wrong_file = store.put(0x70, dag(&unixfs(2, b"", 4, &[3]), &[(child, "", 3)]));
    let directory = store.put(0x70, dag(&unixfs(1, b"", 0, &[]), &[]));
    let malformed = store.put(0x70, vec![0x0a, 0x08, 0x08]);
    let nonminimal = store.put(0x70, vec![0x8a, 0, 0]);
    let mut duplicate_data = dag(&unixfs(2, b"", 0, &[]), &[]);
    bytes(&mut duplicate_data, 1, &unixfs(2, b"", 0, &[]));
    let duplicate_data = store.put(0x70, duplicate_data);
    let unsupported = store.put(0x71, b"unknown".to_vec());
    let unusual_hash = Cid::new_v1(0x55, Multihash::wrap(0x11, &[0; 20]).unwrap());
    let read = |cid| verify(&store, cid, digest(b"one"), Limits::default(), |_| Ok(()));

    for id in [&invalid_link, &missing_link_size] {
        assert!(matches!(read(id), Err(Error::InvalidLinks)));
    }
    for id in [&wrong_total, &wrong_child, &wrong_file] {
        assert!(matches!(read(id), Err(Error::SizeMismatch)));
    }
    for id in [&malformed, &nonminimal, &duplicate_data] {
        assert!(matches!(read(id), Err(Error::InvalidProtobuf)));
    }
    assert!(matches!(read(&directory), Err(Error::UnsupportedUnixfs)));
    for id in [&unsupported, &unusual_hash] {
        assert!(matches!(read(id), Err(Error::UnsupportedCid)));
    }
}

#[test]
fn independent_block_graph_output_and_time_limits_fail_closed() {
    let fixture = fixtures()
        .into_iter()
        .find(|f| f.name == "multiblock-v0")
        .unwrap();
    let temp = Scratch::new();
    materialize(&fixture, &temp.0);
    let store = Flatfs::open(&temp.0).unwrap();
    let expected = digest(&fixture.payload);
    let full = verify(&store, &fixture.root, expected, Limits::default(), |_| {
        Ok(())
    })
    .unwrap();
    let read = |limits| verify(&store, &fixture.root, expected, limits, |_| Ok(()));
    for limits in [
        Limits {
            block_bytes: 1,
            ..Limits::default()
        },
        Limits {
            encoded_bytes: full.encoded_bytes - 1,
            ..Limits::default()
        },
        Limits {
            output_bytes: full.bytes - 1,
            ..Limits::default()
        },
        Limits {
            nodes: full.nodes - 1,
            ..Limits::default()
        },
        Limits {
            depth: 0,
            ..Limits::default()
        },
    ] {
        assert!(matches!(read(limits), Err(Error::Limit(_))));
    }
    assert!(matches!(
        read(Limits {
            time: Duration::ZERO,
            ..Limits::default()
        }),
        Err(Error::Deadline)
    ));
    assert!(matches!(
        verify(&store, &fixture.root, expected, Limits::default(), |_| {
            Err(io::Error::other("staging output unavailable"))
        }),
        Err(Error::Output(_))
    ));
}

#[cfg(feature = "int07")]
mod int07 {
    use super::*;
    use zos_int07_core::{
        ArtifactRevision, AuthorizedSnapshot, AuthorizedSource, BodyKind, ContentDigest,
        Destination, Error as CoreError, EvidenceOutput, Intent, Journal, OperationState,
        PublicationEligibility, Publisher, ReceiptCandidate, ReceiptEvidence, Recovery, Release,
    };
    use zos_unixfs_readback::UnixfsReceiptEvidence;

    struct Source<'a> {
        binding: &'a ArtifactRevision,
        bytes: &'a [u8],
    }

    impl AuthorizedSource for Source<'_> {
        type Principal = ();
        fn with_revision<T>(
            &self,
            _: &(),
            requested: &ArtifactRevision,
            run: impl FnOnce(AuthorizedSnapshot<'_>) -> Result<T, CoreError>,
        ) -> Result<T, CoreError> {
            if requested != self.binding {
                return Err(CoreError::AccessDenied);
            }
            run(AuthorizedSnapshot {
                binding: self.binding,
                release: Release::ExplicitlyPublic,
                kind: BodyKind::SingleFile,
                bytes: self.bytes,
            })
        }
    }

    struct ReleaseRule<'a>(&'a Intent);
    impl PublicationEligibility for ReleaseRule<'_> {
        fn permits(&self, intent: &Intent) -> Result<bool, CoreError> {
            Ok(intent == self.0)
        }
    }

    struct Provider<'a> {
        original: &'a [u8],
        root: Option<String>,
        submits: AtomicUsize,
    }

    impl Destination for Provider<'_> {
        fn id(&self) -> &str {
            "offline-kubo-fixture"
        }

        fn submit(&self, intent: &Intent, body: &[u8]) -> Result<ReceiptCandidate, CoreError> {
            assert_eq!(body, self.original);
            self.submits.fetch_add(1, Ordering::SeqCst);
            Ok(ReceiptCandidate {
                operation: intent.operation(),
                remote_id: "fixture-receipt".into(),
                local_alias: None,
                witness: intent.artifact().digest,
                ipfs_cid: self.root.clone(),
            })
        }

        fn lookup(&self, _: &Intent) -> Result<Option<ReceiptCandidate>, CoreError> {
            Ok(None)
        }
    }

    struct OriginalEvidence<'a> {
        bytes: &'a [u8],
        expected_root: String,
    }

    impl ReceiptEvidence for OriginalEvidence<'_> {
        fn published_bytes(
            &self,
            intent: &Intent,
            receipt: &ReceiptCandidate,
            out: &mut dyn EvidenceOutput,
        ) -> Result<(), CoreError> {
            if receipt.operation != intent.operation()
                || receipt.remote_id != "fixture-receipt"
                || receipt.ipfs_cid.as_deref() != Some(self.expected_root.as_str())
            {
                return Err(CoreError::InvalidReceipt);
            }
            out.append(self.bytes)
        }

        fn verified_ipfs_bytes(
            &self,
            _: &Intent,
            _: &Cid,
            _: &mut dyn EvidenceOutput,
        ) -> Result<(), CoreError> {
            panic!("UnixFS readback must run through the verifier")
        }
    }

    #[test]
    fn real_unixfs_readback_gates_int07_confirmation() {
        let fixture = fixtures()
            .into_iter()
            .find(|f| f.name == "multiblock-v1")
            .unwrap();
        let temp = Scratch::new();
        materialize(&fixture, &temp.0);
        let provider = Provider {
            original: &fixture.payload,
            root: Some(fixture.root.to_string()),
            submits: AtomicUsize::new(0),
        };
        let binding = ArtifactRevision {
            source: "fixture-source".into(),
            tenant: "fixture-tenant".into(),
            artifact: "fixture-file".into(),
            revision: 1,
            digest: ContentDigest::of(&fixture.payload),
        };
        let intent = Intent::new(binding.clone(), provider.id().into()).unwrap();
        let source = Source {
            binding: &binding,
            bytes: &fixture.payload,
        };
        let rule = ReleaseRule(&intent);
        let evidence = UnixfsReceiptEvidence {
            original: OriginalEvidence {
                bytes: &fixture.payload,
                expected_root: fixture.root.to_string(),
            },
            blocks: Flatfs::open(&temp.0).unwrap(),
            limits: Limits::default(),
        };
        let mut successful = Journal::open(temp.0.join("success.sqlite")).unwrap();
        let caller = Publisher {
            source: &source,
            eligibility: &rule,
            destination: &provider,
            evidence: &evidence,
        };
        let result = caller
            .execute(&mut successful, &(), &intent, Recovery::LookupOnly)
            .unwrap();
        let OperationState::Confirmed(receipt) = result else {
            panic!("complete graph must confirm")
        };
        assert_eq!(receipt.ipfs().unwrap().cid(), &fixture.root);
        assert_eq!(provider.submits.load(Ordering::SeqCst), 1);

        let root_path = cid_path(&temp.0, &fixture.root);
        let child = fixture
            .blocks
            .iter()
            .map(|(path, _)| temp.0.join(path))
            .find(|path| *path != root_path)
            .unwrap();
        fs::remove_file(child).unwrap();
        let mut missing_child = Journal::open(temp.0.join("missing-child.sqlite")).unwrap();
        assert!(matches!(
            caller.execute(&mut missing_child, &(), &intent, Recovery::LookupOnly),
            Err(CoreError::ExternalUnavailable)
        ));
        assert_eq!(
            missing_child.state(&intent).unwrap(),
            Some(OperationState::Unresolved)
        );
        assert_eq!(
            caller
                .execute(
                    &mut missing_child,
                    &(),
                    &intent,
                    Recovery::RetryIdempotently
                )
                .unwrap(),
            OperationState::Unresolved
        );
        assert_eq!(provider.submits.load(Ordering::SeqCst), 2);

        let without_root = Provider {
            original: &fixture.payload,
            root: None,
            submits: AtomicUsize::new(0),
        };
        let mut missing_root = Journal::open(temp.0.join("missing-root.sqlite")).unwrap();
        let caller = Publisher {
            destination: &without_root,
            ..caller
        };
        assert!(matches!(
            caller.execute(&mut missing_root, &(), &intent, Recovery::LookupOnly),
            Err(CoreError::InvalidReceipt)
        ));
        assert_eq!(
            missing_root.state(&intent).unwrap(),
            Some(OperationState::Unresolved)
        );
    }
}

#[test]
fn dag_pb_field_order_and_oversized_length_declarations_are_checked() {
    let mut store = Memory::default();
    let child = store.put(0x55, b"A".to_vec());
    let other = store.put(0x55, b"B".to_vec());
    let data = unixfs(2, b"", 2, &[1, 1]);

    let mut misplaced = Vec::new();
    number(&mut misplaced, 3, 1);
    bytes(&mut misplaced, 1, &child.to_bytes());
    let mut encoded = Vec::new();
    bytes(&mut encoded, 2, &misplaced);
    bytes(&mut encoded, 1, &data);
    let out_of_order = store.put(0x70, encoded);

    let links = |cid: Cid| {
        let mut link = Vec::new();
        bytes(&mut link, 1, &cid.to_bytes());
        link
    };
    let mut interleaved = Vec::new();
    bytes(&mut interleaved, 2, &links(child));
    bytes(&mut interleaved, 1, &data);
    bytes(&mut interleaved, 2, &links(other));
    let interleaved = store.put(0x70, interleaved);

    let mut legacy_order = Vec::new();
    bytes(&mut legacy_order, 1, &data);
    bytes(&mut legacy_order, 2, &links(child));
    bytes(&mut legacy_order, 2, &links(other));
    let legacy_order = store.put(0x70, legacy_order);

    let mut huge = vec![0x0a];
    varint(&mut huge, u64::MAX);
    let huge = store.put(0x70, huge);
    for root in [out_of_order, interleaved, huge] {
        assert!(matches!(
            verify(&store, &root, digest(b"AB"), Limits::default(), |_| Ok(())),
            Err(Error::InvalidProtobuf)
        ));
    }
    assert!(
        verify(
            &store,
            &legacy_order,
            digest(b"AB"),
            Limits::default(),
            |_| Ok(())
        )
        .is_ok()
    );
}

#[test]
fn late_block_source_cannot_emit_after_deadline() {
    struct ReturnsTooLate;

    impl BlockSource for ReturnsTooLate {
        fn read_block(&self, _: &Cid, _: usize, deadline: Instant) -> Result<Vec<u8>, Error> {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            Ok(b"present".to_vec())
        }
    }

    let root = Memory::default().put(0x55, b"present".to_vec());
    let mut output_calls = 0;
    let limits = Limits {
        time: Duration::from_millis(15),
        ..Limits::default()
    };
    assert!(matches!(
        verify(&ReturnsTooLate, &root, digest(b"present"), limits, |_| {
            output_calls += 1;
            Ok(())
        }),
        Err(Error::Deadline)
    ));
    assert_eq!(output_calls, 0);
}
