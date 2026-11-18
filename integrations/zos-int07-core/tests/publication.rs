// SPDX-License-Identifier: Apache-2.0
//! All provider observations in this file are synthetic fixtures.

use cid::{Cid, multihash::Multihash};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Barrier, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
    },
    thread,
};
use zos_int07_core::*;

const BODY: &[u8] = b"<p>authorized public fixture</p>";

fn binding() -> ArtifactRevision {
    ArtifactRevision {
        source: "artifact-source".into(),
        tenant: "org-a".into(),
        artifact: "artifact-a".into(),
        revision: 7,
        digest: ContentDigest::of(BODY),
    }
}

fn intent(binding: ArtifactRevision) -> Intent {
    Intent::new(binding, "public-test-destination".into()).unwrap()
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let suffix = NEXT.fetch_add(1, SeqCst);
            let path =
                std::env::temp_dir().join(format!("zos-int07-{}-{suffix}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test directory: {error}"),
            }
        }
    }

    fn open(&self) -> Journal {
        Journal::open(self.0.join("journal.sqlite")).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Source {
    binding: ArtifactRevision,
    release: Release,
    kind: BodyKind,
    body: &'static [u8],
}
impl Source {
    fn public() -> Self {
        Self {
            binding: binding(),
            release: Release::ExplicitlyPublic,
            kind: BodyKind::SingleFile,
            body: BODY,
        }
    }
}
impl AuthorizedSource for Source {
    type Principal = str;
    fn with_revision<T>(
        &self,
        principal: &str,
        _requested: &ArtifactRevision,
        use_snapshot: impl FnOnce(AuthorizedSnapshot<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if principal != "authorized" {
            return Err(Error::AccessDenied);
        }
        use_snapshot(AuthorizedSnapshot {
            binding: &self.binding,
            release: self.release,
            kind: self.kind,
            bytes: self.body,
        })
    }
}

#[derive(Default)]
struct Remote {
    receipt: Mutex<Option<ReceiptCandidate>>,
    submits: AtomicUsize,
    lookups: AtomicUsize,
    timeout: AtomicBool,
    discard: AtomicBool,
    cid: Option<String>,
    override_operation: Option<OperationId>,
    block: Option<(Barrier, Barrier)>,
}
impl Remote {
    fn candidate(&self, intent: &Intent) -> ReceiptCandidate {
        ReceiptCandidate {
            operation: self
                .override_operation
                .unwrap_or_else(|| intent.operation()),
            remote_id: "fixture-remote-id".into(),
            local_alias: Some(intent.artifact().digest.pastebin_alias()),
            witness: intent.artifact().digest,
            ipfs_cid: self.cid.clone(),
        }
    }
}
impl Destination for Remote {
    fn id(&self) -> &str {
        "public-test-destination"
    }
    fn submit(&self, intent: &Intent, body: &[u8]) -> Result<ReceiptCandidate, Error> {
        self.submits.fetch_add(1, SeqCst);
        assert_eq!(body, BODY);
        if let Some((entered, release)) = &self.block {
            entered.wait();
            release.wait();
        }
        if self.discard.load(SeqCst) {
            return Err(Error::ExternalUnavailable);
        }
        let receipt = self.candidate(intent);
        *self.receipt.lock().unwrap() = Some(receipt.clone());
        if self.timeout.load(SeqCst) {
            return Err(Error::ExternalUnavailable);
        }
        Ok(receipt)
    }
    fn lookup(&self, _: &Intent) -> Result<Option<ReceiptCandidate>, Error> {
        self.lookups.fetch_add(1, SeqCst);
        Ok(self.receipt.lock().unwrap().clone())
    }

    fn durable_retry(&self) -> Option<&dyn DurableRetry> {
        Some(self)
    }
}

impl DurableRetry for Remote {
    fn retry_idempotently(
        &self,
        intent: &Intent,
        body: &[u8],
    ) -> Result<Option<ReceiptCandidate>, Error> {
        assert_eq!(body, BODY);
        // The fixture's retained receipt and atomic insertion model a provider ledger.
        Ok(Some(
            self.receipt
                .lock()
                .unwrap()
                .get_or_insert_with(|| self.candidate(intent))
                .clone(),
        ))
    }
}

struct Eligibility(bool);
impl PublicationEligibility for Eligibility {
    fn permits(&self, _: &Intent) -> Result<bool, Error> {
        Ok(self.0)
    }
}

struct Evidence {
    published: Option<&'static [u8]>,
    ipfs: Option<&'static [u8]>,
}
impl ReceiptEvidence for Evidence {
    fn published_bytes(
        &self,
        _: &Intent,
        _: &ReceiptCandidate,
        out: &mut dyn EvidenceOutput,
    ) -> Result<(), Error> {
        out.append(self.published.ok_or(Error::InvalidReceipt)?)
    }
    fn verified_ipfs_bytes(
        &self,
        _: &Intent,
        _: &Cid,
        out: &mut dyn EvidenceOutput,
    ) -> Result<(), Error> {
        out.append(self.ipfs.ok_or(Error::InvalidReceipt)?)
    }
}
const EVIDENCE: Evidence = Evidence {
    published: Some(BODY),
    ipfs: Some(BODY),
};

fn publisher<'a>(
    source: &'a Source,
    eligibility: &'a Eligibility,
    remote: &'a Remote,
    evidence: &'a Evidence,
) -> Publisher<'a, Source, Eligibility, Remote, Evidence> {
    Publisher {
        source,
        eligibility,
        destination: remote,
        evidence,
    }
}

#[test]
fn denies_non_public_wrong_tenant_revision_bytes_and_bundles_before_dispatch() {
    let db = Scratch::new();
    let mut journal = db.open();
    let remote = Remote::default();
    let binding = binding();
    let intent = intent(binding.clone());
    let mut source = Source::public();

    let request = |source: &Source, permitted, principal: &str, journal: &mut Journal| {
        publisher(source, &Eligibility(permitted), &remote, &EVIDENCE).execute(
            journal,
            principal,
            &intent,
            Recovery::LookupOnly,
        )
    };
    assert!(matches!(
        request(&source, true, "denied", &mut journal),
        Err(Error::AccessDenied)
    ));
    assert!(matches!(
        request(&source, false, "authorized", &mut journal),
        Err(Error::Ineligible)
    ));
    for release in [Release::Private, Release::Unclassified] {
        source.release = release;
        assert!(matches!(
            request(&source, true, "authorized", &mut journal),
            Err(Error::Ineligible)
        ));
    }
    source.release = Release::ExplicitlyPublic;
    for mismatched in [
        ArtifactRevision {
            tenant: "org-b".into(),
            ..binding.clone()
        },
        ArtifactRevision {
            revision: 8,
            ..binding.clone()
        },
        ArtifactRevision {
            source: "other-instance".into(),
            ..binding.clone()
        },
        ArtifactRevision {
            artifact: "other-artifact".into(),
            ..binding.clone()
        },
    ] {
        source.binding = mismatched;
        assert!(matches!(
            request(&source, true, "authorized", &mut journal),
            Err(Error::InvalidBinding)
        ));
    }
    source.binding = binding;
    source.body = b"wrong content";
    assert!(matches!(
        request(&source, true, "authorized", &mut journal),
        Err(Error::DigestMismatch)
    ));
    source.kind = BodyKind::Bundle;
    assert!(matches!(
        request(&source, true, "authorized", &mut journal),
        Err(Error::UnsupportedBody)
    ));
    assert_eq!(journal.state(&intent).unwrap(), None);
    assert_eq!(remote.submits.load(SeqCst), 0);
    assert_eq!(remote.lookups.load(SeqCst), 0);
}

#[test]
fn reconciles_lost_ack_after_reopen_and_reauthorizes_confirmed_receipt() {
    let db = Scratch::new();
    let intent = intent(binding());
    let remote = Remote::default();
    remote.timeout.store(true, SeqCst);
    let mut source = Source::public();
    let permit = Eligibility(true);
    let caller = publisher(&source, &permit, &remote, &EVIDENCE);
    assert!(matches!(
        caller.execute(&mut db.open(), "authorized", &intent, Recovery::LookupOnly),
        Err(Error::ExternalUnavailable)
    ));
    let mut reopened = db.open();
    assert_eq!(reopened.pending().unwrap(), std::slice::from_ref(&intent));
    let confirmed = caller
        .execute(&mut reopened, "authorized", &intent, Recovery::LookupOnly)
        .unwrap();
    let OperationState::Confirmed(receipt) = &confirmed else {
        panic!("expected confirmed receipt")
    };
    assert_eq!(receipt.remote_id(), "fixture-remote-id");
    assert_eq!(receipt.ipfs(), None);
    assert!(reopened.pending().unwrap().is_empty());
    drop(reopened);
    assert_eq!(
        caller
            .execute(&mut db.open(), "authorized", &intent, Recovery::LookupOnly)
            .unwrap(),
        confirmed
    );
    source.release = Release::Private;
    let caller = publisher(&source, &permit, &remote, &EVIDENCE);
    assert!(matches!(
        caller.execute(&mut db.open(), "authorized", &intent, Recovery::LookupOnly),
        Err(Error::Ineligible)
    ));
    assert_eq!(remote.submits.load(SeqCst), 1);
    assert_eq!(remote.lookups.load(SeqCst), 1);
}

#[test]
fn failed_dispatch_stays_unresolved_and_requires_safe_recovery() {
    let db = Scratch::new();
    let intent = intent(binding());
    let source = Source::public();
    let remote = Remote::default();
    remote.discard.store(true, SeqCst);
    let permit = Eligibility(true);
    let caller = publisher(&source, &permit, &remote, &EVIDENCE);
    assert!(matches!(
        caller.execute(&mut db.open(), "authorized", &intent, Recovery::LookupOnly),
        Err(Error::ExternalUnavailable)
    ));
    assert_eq!(
        caller
            .execute(&mut db.open(), "authorized", &intent, Recovery::LookupOnly)
            .unwrap(),
        OperationState::Unresolved
    );
    assert!(matches!(
        caller.execute(
            &mut db.open(),
            "authorized",
            &intent,
            Recovery::RetryIdempotently
        ),
        Ok(OperationState::Confirmed(_))
    ));
    assert_eq!(remote.submits.load(SeqCst), 1);
}

#[test]
fn receipt_cannot_be_confirmed_from_echoed_metadata_wrong_payload_or_pseudo_cid() {
    let intent = intent(binding());
    let source = Source::public();
    let eligibility = Eligibility(true);
    for (candidate_cid, observed, expected_invalid) in [
        (None, None, true),
        (None, Some(b"wrong content".as_slice()), false),
        (
            Some(intent.artifact().digest.pastebin_alias().0),
            Some(BODY),
            true,
        ),
    ] {
        let db = Scratch::new();
        let remote = Remote {
            cid: candidate_cid,
            ..Remote::default()
        };
        let evidence = Evidence {
            published: observed,
            ipfs: Some(BODY),
        };
        let error = publisher(&source, &eligibility, &remote, &evidence)
            .execute(&mut db.open(), "authorized", &intent, Recovery::LookupOnly)
            .unwrap_err();
        assert!(if expected_invalid {
            matches!(error, Error::InvalidReceipt)
        } else {
            matches!(error, Error::DigestMismatch)
        });
        assert_eq!(
            db.open().state(&intent).unwrap(),
            Some(OperationState::Unresolved)
        );
    }
}

#[test]
fn verified_raw_cid_persists_and_wrong_multihash_is_rejected() {
    let intent = intent(binding());
    let source = Source::public();
    let eligibility = Eligibility(true);
    for digest in [intent.artifact().digest, ContentDigest::of(b"other")] {
        let db = Scratch::new();
        let root = Cid::new_v1(0x55, Multihash::wrap(0x12, &digest.0).unwrap());
        let remote = Remote {
            cid: Some(root.to_string()),
            ..Remote::default()
        };
        let result = publisher(&source, &eligibility, &remote, &EVIDENCE).execute(
            &mut db.open(),
            "authorized",
            &intent,
            Recovery::LookupOnly,
        );
        if digest == intent.artifact().digest {
            let Some(OperationState::Confirmed(receipt)) = db.open().state(&intent).unwrap() else {
                panic!("expected confirmed IPFS fixture")
            };
            assert_eq!(receipt.ipfs().unwrap().cid(), &root);
            assert_eq!(result.unwrap(), OperationState::Confirmed(receipt));
        } else {
            assert!(matches!(result, Err(Error::InvalidReceipt)));
            assert_eq!(
                db.open().state(&intent).unwrap(),
                Some(OperationState::Unresolved)
            );
        }
    }
}

#[test]
fn concurrent_connections_dispatch_once_and_sqlite_is_unlocked_during_submission() {
    let db = Scratch::new();
    let intent = intent(binding());
    let remote = Remote {
        block: Some((Barrier::new(2), Barrier::new(2))),
        ..Remote::default()
    };
    let (entered, release) = remote.block.as_ref().unwrap();
    let source = Source::public();
    let permit = Eligibility(true);
    let caller = publisher(&source, &permit, &remote, &EVIDENCE);
    let mut second_connection = db.open();
    thread::scope(|scope| {
        let dispatched = scope
            .spawn(|| caller.execute(&mut db.open(), "authorized", &intent, Recovery::LookupOnly));
        entered.wait();
        let competing = caller.execute(
            &mut second_connection,
            "authorized",
            &intent,
            Recovery::LookupOnly,
        );
        release.wait();
        assert_eq!(competing.unwrap(), OperationState::Unresolved);
        assert!(matches!(
            dispatched.join().unwrap(),
            Ok(OperationState::Confirmed(_))
        ));
    });
    assert_eq!(remote.submits.load(SeqCst), 1);
}

#[test]
fn sqlite_errors_are_returned() {
    let db = Scratch::new();
    assert!(matches!(
        Journal::open(db.0.join("missing").join("db.sqlite")),
        Err(Error::Sqlite(_))
    ));
    assert!(matches!(
        Journal::open(":memory:"),
        Err(Error::JournalConflict)
    ));
    let foreign = db.0.join("foreign.sqlite");
    rusqlite::Connection::open(&foreign)
        .unwrap()
        .execute("CREATE TABLE data(x INTEGER)", [])
        .unwrap();
    assert!(matches!(
        Journal::open(foreign),
        Err(Error::JournalConflict)
    ));
}

#[test]
fn refuses_a_receipt_for_another_operation_with_the_same_body() {
    let revision = binding();
    let unrelated = Intent::new(revision.clone(), "elsewhere".into()).unwrap();
    let intent = intent(revision);
    let remote = Remote {
        override_operation: Some(unrelated.operation()),
        ..Remote::default()
    };
    let db = Scratch::new();
    let source = Source::public();
    let caller = publisher(&source, &Eligibility(true), &remote, &EVIDENCE);
    assert!(matches!(
        caller.execute(&mut db.open(), "authorized", &intent, Recovery::LookupOnly),
        Err(Error::InvalidReceipt)
    ));
    assert_eq!(
        db.open().state(&intent).unwrap(),
        Some(OperationState::Unresolved)
    );
}
