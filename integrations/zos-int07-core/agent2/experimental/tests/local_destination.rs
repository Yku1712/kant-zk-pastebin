// SPDX-License-Identifier: Apache-2.0

use rusqlite::Connection;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering::SeqCst},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread,
    time::Duration,
};
use zos_int07_core::{
    ArtifactRevision, AuthorizedSnapshot, AuthorizedSource, BodyKind, ContentDigest, Destination,
    Error, Intent, Journal, OperationState, PublicationEligibility, Publisher, ReceiptCandidate,
    ReceiptEvidence, Recovery, Release,
};
use zos_int07_local_destination::{LocalDestination, MAX_BYTES};

const BODY: &[u8] = b"experimental exact bytes\xff\r\n";

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "zos-agent2-local-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, SeqCst)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("could not create disposable directory: {error}"),
            }
        }
    }

    fn destination(&self) -> LocalDestination {
        LocalDestination::open(self.0.join("provider.sqlite"), "unit").unwrap()
    }

    fn journal(&self) -> Journal {
        Journal::open(self.0.join("journal.sqlite")).unwrap()
    }

    fn stored_count(&self) -> usize {
        Connection::open(self.0.join("provider.sqlite"))
            .unwrap()
            .query_row("SELECT count(*) FROM published_payload", [], |row| {
                row.get(0)
            })
            .unwrap()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn intent(destination: &str, body: &[u8]) -> Intent {
    Intent::new(
        ArtifactRevision {
            source: "local-source".into(),
            tenant: "local-tenant".into(),
            artifact: "local-artifact".into(),
            revision: 1,
            digest: ContentDigest::of(body),
        },
        destination.to_owned(),
    )
    .unwrap()
}

struct Snapshot(ArtifactRevision);

impl AuthorizedSource for Snapshot {
    type Principal = ();
    fn with_revision<T>(
        &self,
        _: &(),
        requested: &ArtifactRevision,
        run: impl FnOnce(AuthorizedSnapshot<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if requested != &self.0 {
            return Err(Error::AccessDenied);
        }
        run(AuthorizedSnapshot {
            binding: &self.0,
            release: Release::ExplicitlyPublic,
            kind: BodyKind::SingleFile,
            bytes: BODY,
        })
    }
}

struct Policy(Intent);
impl PublicationEligibility for Policy {
    fn permits(&self, intent: &Intent) -> Result<bool, Error> {
        Ok(&self.0 == intent)
    }
}

struct LoseResponse<'a>(&'a LocalDestination);
impl Destination for LoseResponse<'_> {
    fn id(&self) -> &str {
        self.0.id()
    }

    fn submit(&self, intent: &Intent, body: &[u8]) -> Result<ReceiptCandidate, Error> {
        self.0.submit(intent, body)?;
        Err(Error::ExternalUnavailable)
    }

    fn lookup(&self, intent: &Intent) -> Result<Option<ReceiptCandidate>, Error> {
        self.0.lookup(intent)
    }
}

#[test]
fn committed_provider_write_survives_lost_response_and_reopens_for_readback() {
    let temp = Temp::new();
    let local = temp.destination();
    let intent = intent(local.id(), BODY);
    let source = Snapshot(intent.artifact().clone());
    let policy = Policy(intent.clone());
    let first = Publisher {
        source: &source,
        eligibility: &policy,
        destination: &LoseResponse(&local),
        evidence: &local,
    };
    assert!(matches!(
        first.execute(&mut temp.journal(), &(), &intent, Recovery::LookupOnly),
        Err(Error::ExternalUnavailable)
    ));
    assert_eq!(temp.stored_count(), 1);
    assert_eq!(
        temp.journal().state(&intent).unwrap(),
        Some(OperationState::Unresolved)
    );
    let reopened = temp.destination();
    let mut journal = temp.journal();
    let caller = Publisher {
        source: &source,
        eligibility: &policy,
        destination: &reopened,
        evidence: &reopened,
    };
    let receipt = reopened.lookup(&intent).unwrap().unwrap();
    assert_eq!(reopened.published_bytes(&intent, &receipt).unwrap(), BODY);
    assert!(matches!(
        caller.execute(&mut journal, &(), &intent, Recovery::LookupOnly),
        Ok(OperationState::Confirmed(confirmed)) if confirmed.remote_id() == receipt.remote_id
    ));
    assert_eq!(temp.stored_count(), 1);
}

struct Overlap<'a> {
    inner: &'a LocalDestination,
    entered: SyncSender<()>,
    finish: Mutex<Receiver<()>>,
    retries: AtomicUsize,
}

impl Destination for Overlap<'_> {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn submit(&self, intent: &Intent, body: &[u8]) -> Result<ReceiptCandidate, Error> {
        self.entered
            .send(())
            .map_err(|_| Error::ExternalUnavailable)?;
        self.finish
            .lock()
            .map_err(|_| Error::ExternalUnavailable)?
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| Error::ExternalUnavailable)?;
        self.inner.submit(intent, body)
    }

    fn lookup(&self, intent: &Intent) -> Result<Option<ReceiptCandidate>, Error> {
        self.inner.lookup(intent)
    }

    fn retry_idempotently(
        &self,
        intent: &Intent,
        body: &[u8],
    ) -> Result<Option<ReceiptCandidate>, Error> {
        self.retries.fetch_add(1, SeqCst);
        self.inner.retry_idempotently(intent, body)
    }
}

#[test]
fn concurrent_first_submission_and_explicit_retry_commit_one_provider_record() {
    let temp = Temp::new();
    let local = temp.destination();
    let (started, start_observed) = sync_channel(1);
    let (resume, resumed) = sync_channel(1);
    let both = Overlap {
        inner: &local,
        entered: started,
        finish: Mutex::new(resumed),
        retries: AtomicUsize::new(0),
    };
    let intent = intent(local.id(), BODY);
    let source = Snapshot(intent.artifact().clone());
    let caller = Publisher {
        source: &source,
        eligibility: &Policy(intent.clone()),
        destination: &both,
        evidence: &local,
    };
    let mut retry_journal = temp.journal();
    let mut first_journal = temp.journal();
    thread::scope(|scope| {
        let first =
            scope.spawn(|| caller.execute(&mut first_journal, &(), &intent, Recovery::LookupOnly));
        start_observed
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        let retry = caller.execute(
            &mut retry_journal,
            &(),
            &intent,
            Recovery::RetryIdempotently,
        );
        resume.send(()).unwrap();
        let first = first.join().unwrap().unwrap();
        assert!(matches!(first, OperationState::Confirmed(_)));
        assert_eq!(first, retry.unwrap());
    });
    assert_eq!(both.retries.load(SeqCst), 1);
    assert_eq!(temp.stored_count(), 1);
}

#[test]
fn provider_limits_input_and_binds_readback_to_operation_and_instance() {
    let temp = Temp::new();
    let local = temp.destination();
    let original = intent(local.id(), BODY);
    let other = intent("fixture:other", BODY);
    assert!(matches!(local.lookup(&other), Err(Error::InvalidBinding)));
    assert!(matches!(
        LocalDestination::open(temp.0.join("provider.sqlite"), "other"),
        Err(Error::InvalidBinding)
    ));
    assert!(matches!(
        local.submit(&intent(local.id(), b""), b""),
        Err(Error::UnsupportedBody)
    ));
    let oversized = vec![b'x'; MAX_BYTES + 1];
    assert!(matches!(
        local.submit(&intent(local.id(), &oversized), &oversized),
        Err(Error::UnsupportedBody)
    ));
    assert!(matches!(
        local.submit(&original, b"wrong"),
        Err(Error::DigestMismatch)
    ));
    let receipt = local.submit(&original, BODY).unwrap();
    let another_scope = Intent::new(
        ArtifactRevision {
            tenant: "second-tenant".into(),
            ..original.artifact().clone()
        },
        local.id().to_owned(),
    )
    .unwrap();
    assert!(local.lookup(&another_scope).unwrap().is_none());
    assert!(matches!(
        local.published_bytes(&another_scope, &receipt),
        Err(Error::InvalidReceipt)
    ));
    let mut changed = receipt;
    changed.remote_id = "unrelated-record".into();
    assert!(matches!(
        local.published_bytes(&original, &changed),
        Err(Error::InvalidReceipt)
    ));
    assert_eq!(temp.stored_count(), 1);
}
