// SPDX-License-Identifier: Apache-2.0
//! Independent tests of the frozen core. Provider and authorization observations are fixtures.

use cid::{Cid, multihash::Multihash};
use rusqlite::{Connection, ErrorCode};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    time::Duration,
};
use zos_int07_core::{
    ArtifactRevision, AuthorizedSnapshot, AuthorizedSource, BodyKind, ContentDigest, Destination,
    Error, Intent, Journal, LocalAlias, OperationState, PublicationEligibility, Publisher,
    ReceiptCandidate, ReceiptEvidence, Recovery, Release,
};

const PAYLOAD: &[u8] = b"independently observed test payload";
const DESTINATION: &str = "agent2-fixture";
const TEST_TIMEOUT: Duration = Duration::from_secs(20);

fn revision() -> ArtifactRevision {
    ArtifactRevision {
        source: "source-1".into(),
        tenant: "tenant-1".into(),
        artifact: "artifact-1".into(),
        revision: 42,
        digest: ContentDigest::of(PAYLOAD),
    }
}

fn intent() -> Intent {
    Intent::new(revision(), DESTINATION.into()).unwrap()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "zos-agent2-{}-{}",
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

    fn journal_path(&self) -> PathBuf {
        self.0.join("journal.sqlite")
    }

    fn journal(&self) -> Journal {
        Journal::open(self.journal_path()).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Grant<'a> {
    authenticated: bool,
    epoch: u64,
    tenant: &'a str,
}

impl Grant<'_> {
    const CURRENT: Self = Self {
        authenticated: true,
        epoch: 2,
        tenant: "tenant-1",
    };
}

struct Source {
    revision: ArtifactRevision,
    release: Release,
    body: Vec<u8>,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            revision: revision(),
            release: Release::ExplicitlyPublic,
            body: PAYLOAD.to_vec(),
        }
    }
}

impl AuthorizedSource for Source {
    type Principal = Grant<'static>;

    fn with_revision<T>(
        &self,
        grant: &Self::Principal,
        requested: &ArtifactRevision,
        use_snapshot: impl FnOnce(AuthorizedSnapshot<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if !grant.authenticated || grant.epoch != 2 || requested.tenant != grant.tenant {
            return Err(Error::AccessDenied);
        }
        use_snapshot(AuthorizedSnapshot {
            binding: &self.revision,
            release: self.release,
            kind: BodyKind::SingleFile,
            bytes: &self.body,
        })
    }
}

struct ReleaseRule(Intent);

impl PublicationEligibility for ReleaseRule {
    fn permits(&self, request: &Intent) -> Result<bool, Error> {
        Ok(*request == self.0)
    }
}

struct DispatchGate {
    entered: SyncSender<()>,
    resume: Mutex<Receiver<()>>,
}

#[derive(Default)]
struct Provider {
    submits: AtomicUsize,
    lookups: AtomicUsize,
    submission: Option<ReceiptCandidate>,
    observation: Option<ReceiptCandidate>,
    gate: Option<DispatchGate>,
    pause_submit: Option<PathBuf>,
}

impl Destination for Provider {
    fn id(&self) -> &str {
        DESTINATION
    }

    fn submit(&self, _: &Intent, _: &[u8]) -> Result<ReceiptCandidate, Error> {
        self.submits.fetch_add(1, SeqCst);
        if let Some(directory) = &self.pause_submit {
            park(directory);
        }
        if let Some(gate) = &self.gate {
            gate.entered
                .send(())
                .map_err(|_| Error::ExternalUnavailable)?;
            gate.resume
                .lock()
                .map_err(|_| Error::ExternalUnavailable)?
                .recv_timeout(TEST_TIMEOUT)
                .map_err(|_| Error::ExternalUnavailable)?;
        }
        self.submission.clone().ok_or(Error::ExternalUnavailable)
    }

    fn lookup(&self, _: &Intent) -> Result<Option<ReceiptCandidate>, Error> {
        self.lookups.fetch_add(1, SeqCst);
        Ok(self.observation.clone())
    }
}

struct Evidence<'a> {
    bytes: &'a [u8],
    ipfs: Option<&'a [u8]>,
    pause_at: Option<&'a Path>,
}

impl Evidence<'_> {
    const VALID: Self = Self {
        bytes: PAYLOAD,
        ipfs: None,
        pause_at: None,
    };
}

impl ReceiptEvidence for Evidence<'_> {
    fn published_bytes(&self, _: &Intent, _: &ReceiptCandidate) -> Result<Vec<u8>, Error> {
        if let Some(directory) = self.pause_at {
            park(directory);
        }
        Ok(self.bytes.to_vec())
    }

    fn verified_ipfs_bytes(&self, _: &Intent, _: &Cid) -> Result<Vec<u8>, Error> {
        self.ipfs
            .map(<[u8]>::to_vec)
            .ok_or(Error::ExternalUnavailable)
    }
}

fn receipt(intent: &Intent) -> ReceiptCandidate {
    ReceiptCandidate {
        operation: intent.operation(),
        remote_id: "confirmed-provider-fixture".into(),
        local_alias: Some(intent.artifact().digest.pastebin_alias()),
        witness: intent.artifact().digest,
        ipfs_cid: None,
    }
}

fn publisher<'a>(
    source: &'a Source,
    policy: &'a ReleaseRule,
    provider: &'a Provider,
    evidence: &'a Evidence<'a>,
) -> Publisher<'a, Source, ReleaseRule, Provider, Evidence<'a>> {
    Publisher {
        source,
        eligibility: policy,
        destination: provider,
        evidence,
    }
}

#[test]
fn stale_or_unauthenticated_grants_do_not_dispatch_or_reconcile() {
    let scratch = Scratch::new();
    let mut journal = scratch.journal();
    let intent = intent();
    let source = Source::default();
    let policy = ReleaseRule(intent.clone());
    let provider = Provider {
        observation: Some(receipt(&intent)),
        ..Provider::default()
    };
    let caller = publisher(&source, &policy, &provider, &Evidence::VALID);
    for grant in [
        Grant {
            authenticated: false,
            ..Grant::CURRENT
        },
        Grant {
            epoch: 1,
            ..Grant::CURRENT
        },
        Grant {
            tenant: "tenant-2",
            ..Grant::CURRENT
        },
    ] {
        assert!(matches!(
            caller.execute(&mut journal, &grant, &intent, Recovery::LookupOnly),
            Err(Error::AccessDenied)
        ));
    }
    assert_eq!(journal.state(&intent).unwrap(), None);
    assert!(journal.pending().unwrap().is_empty());
    assert_eq!(provider.submits.load(SeqCst), 0);
    assert!(matches!(
        caller.execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly),
        Err(Error::ExternalUnavailable)
    ));
    let stale = Grant {
        epoch: 1,
        ..Grant::CURRENT
    };
    assert!(matches!(
        caller.execute(&mut journal, &stale, &intent, Recovery::RetryIdempotently),
        Err(Error::AccessDenied)
    ));
    assert_eq!(provider.lookups.load(SeqCst), 0);
    assert_eq!(
        journal.state(&intent).unwrap(),
        Some(OperationState::Unresolved)
    );
}

#[test]
fn release_and_snapshot_must_match_full_intent_before_dispatch() {
    let scratch = Scratch::new();
    let mut journal = scratch.journal();
    let intent = intent();
    let source = Source::default();
    let provider = Provider::default();
    let other_destination = Intent::new(revision(), "another-destination".into()).unwrap();
    for mismatch in [
        ArtifactRevision {
            source: "source-2".into(),
            ..revision()
        },
        ArtifactRevision {
            tenant: "tenant-2".into(),
            ..revision()
        },
        ArtifactRevision {
            artifact: "artifact-2".into(),
            ..revision()
        },
        ArtifactRevision {
            revision: 41,
            ..revision()
        },
        ArtifactRevision {
            digest: ContentDigest::of(b"different"),
            ..revision()
        },
    ] {
        let wrong_policy = ReleaseRule(Intent::new(mismatch.clone(), DESTINATION.into()).unwrap());
        assert!(matches!(
            publisher(&source, &wrong_policy, &provider, &Evidence::VALID).execute(
                &mut journal,
                &Grant::CURRENT,
                &intent,
                Recovery::LookupOnly
            ),
            Err(Error::Ineligible)
        ));
        let wrong_source = Source {
            revision: mismatch,
            ..Source::default()
        };
        assert!(matches!(
            publisher(
                &wrong_source,
                &ReleaseRule(intent.clone()),
                &provider,
                &Evidence::VALID
            )
            .execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly),
            Err(Error::InvalidBinding)
        ));
    }
    let policy = ReleaseRule(other_destination.clone());
    assert!(matches!(
        publisher(&source, &policy, &provider, &Evidence::VALID).execute(
            &mut journal,
            &Grant::CURRENT,
            &intent,
            Recovery::LookupOnly
        ),
        Err(Error::Ineligible)
    ));
    assert!(matches!(
        publisher(&source, &policy, &provider, &Evidence::VALID).execute(
            &mut journal,
            &Grant::CURRENT,
            &other_destination,
            Recovery::LookupOnly
        ),
        Err(Error::InvalidBinding)
    ));
    assert_eq!(journal.state(&intent).unwrap(), None);
    assert!(journal.pending().unwrap().is_empty());
    assert_eq!(provider.submits.load(SeqCst), 0);
}

#[test]
fn identifier_and_witness_boundaries_are_deterministic() {
    for invalid in ["".into(), " ".into(), "line\nbreak".into(), "é".repeat(257)] {
        assert!(matches!(
            Intent::new(revision(), invalid),
            Err(Error::InvalidBinding)
        ));
    }
    assert!(Intent::new(revision(), "é".repeat(256)).is_ok());
    assert!(
        Intent::new(
            ArtifactRevision {
                revision: 0,
                ..revision()
            },
            DESTINATION.into()
        )
        .is_err()
    );
    assert!("a".repeat(64).parse::<ContentDigest>().is_ok());
    assert!("A".repeat(64).parse::<ContentDigest>().is_err());
}

#[test]
fn corrupt_or_unavailable_database_returns_errors_before_dispatch() {
    let scratch = Scratch::new();
    fs::write(scratch.journal_path(), b"invalid SQLite file").unwrap();
    assert!(matches!(
        Journal::open(scratch.journal_path()),
        Err(Error::Sqlite(error)) if error.sqlite_error_code() == Some(ErrorCode::NotADatabase)
    ));
    assert!(matches!(
        Journal::open(scratch.0.join("absent").join("journal")),
        Err(Error::Sqlite(error)) if error.sqlite_error_code() == Some(ErrorCode::CannotOpen)
    ));
}

#[test]
fn sqlite_writer_lock_times_out_without_dispatch() {
    let scratch = Scratch::new();
    let mut journal = scratch.journal();
    let locker = Connection::open(scratch.journal_path()).unwrap();
    locker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let provider = Provider::default();
    let intent = intent();
    let result = publisher(
        &Source::default(),
        &ReleaseRule(intent.clone()),
        &provider,
        &Evidence::VALID,
    )
    .execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly);
    assert!(matches!(
        result,
        Err(Error::Sqlite(error)) if error.sqlite_error_code() == Some(ErrorCode::DatabaseBusy)
    ));
    locker.execute_batch("ROLLBACK").unwrap();
    assert_eq!(journal.state(&intent).unwrap(), None);
    assert_eq!(provider.submits.load(SeqCst), 0);
}

#[test]
fn malformed_receipts_and_incomplete_readback_stay_unresolved() {
    let intent = intent();
    let dag = Cid::new_v1(
        0x70,
        Multihash::wrap(0x12, &ContentDigest::of(b"root block").0).unwrap(),
    );
    let mut cases = Vec::new();
    let mut invalid = receipt(&intent);
    invalid.remote_id = " ".into();
    cases.push((invalid, Evidence::VALID, Error::InvalidReceipt));
    let mut invalid = receipt(&intent);
    invalid.witness = ContentDigest::of(b"different bytes");
    cases.push((invalid, Evidence::VALID, Error::InvalidReceipt));
    let mut invalid = receipt(&intent);
    invalid.local_alias = Some(LocalAlias("wrong-alias".into()));
    cases.push((invalid, Evidence::VALID, Error::InvalidReceipt));
    let wrong_scope = Intent::new(
        ArtifactRevision {
            revision: 41,
            ..revision()
        },
        DESTINATION.into(),
    )
    .unwrap();
    cases.push((
        receipt(&wrong_scope),
        Evidence::VALID,
        Error::InvalidReceipt,
    ));
    let mut invalid = receipt(&intent);
    invalid.ipfs_cid = Some("not-a-cid".into());
    cases.push((invalid, Evidence::VALID, Error::InvalidReceipt));
    let mut needs_dag = receipt(&intent);
    needs_dag.ipfs_cid = Some(dag.to_string());
    cases.push((
        needs_dag.clone(),
        Evidence::VALID,
        Error::ExternalUnavailable,
    ));
    cases.push((
        needs_dag,
        Evidence {
            ipfs: Some(&PAYLOAD[..5]),
            ..Evidence::VALID
        },
        Error::DigestMismatch,
    ));
    for (candidate, evidence, expected) in cases {
        let scratch = Scratch::new();
        let mut journal = scratch.journal();
        let provider = Provider {
            submission: Some(candidate),
            ..Provider::default()
        };
        let observed = publisher(
            &Source::default(),
            &ReleaseRule(intent.clone()),
            &provider,
            &evidence,
        )
        .execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly)
        .unwrap_err();
        assert_eq!(
            std::mem::discriminant(&observed),
            std::mem::discriminant(&expected)
        );
        assert_eq!(
            journal.state(&intent).unwrap(),
            Some(OperationState::Unresolved)
        );
        assert_eq!(provider.submits.load(SeqCst), 1);
    }
}

fn park(path: &Path) -> ! {
    fs::write(path.join("ready"), b"ready").unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
#[ignore = "child entry point for process-kill tests"]
fn crash_child() {
    let Some(directory) = std::env::var_os("ZOS_AGENT2_CRASH_DIR").map(PathBuf::from) else {
        return;
    };
    let phase = std::env::var("ZOS_AGENT2_CRASH_PHASE").unwrap();
    assert!(matches!(
        phase.as_str(),
        "claimed" | "confirming" | "confirmed"
    ));
    let mut journal = Journal::open(directory.join("journal.sqlite")).unwrap();
    let intent = intent();
    let provider = Provider {
        submission: Some(receipt(&intent)),
        observation: Some(receipt(&intent)),
        pause_submit: (phase == "claimed").then(|| directory.clone()),
        ..Provider::default()
    };
    let evidence = Evidence {
        pause_at: (phase == "confirming").then_some(directory.as_path()),
        ..Evidence::VALID
    };
    let state = publisher(
        &Source::default(),
        &ReleaseRule(intent.clone()),
        &provider,
        &evidence,
    )
    .execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly)
    .unwrap();
    assert_eq!(phase, "confirmed");
    assert!(matches!(state, OperationState::Confirmed(_)));
    park(&directory);
}

#[cfg(unix)]
fn kill_child_at_marker(scratch: &Scratch, phase: &str) {
    use std::{
        os::unix::process::ExitStatusExt,
        process::{Command, Stdio},
        thread,
        time::Instant,
    };
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "crash_child"])
        .env("ZOS_AGENT2_CRASH_DIR", &scratch.0)
        .env("ZOS_AGENT2_CRASH_PHASE", phase)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let ready = loop {
        if scratch.0.join("ready").exists() {
            break true;
        }
        if child.try_wait().unwrap().is_some() || start.elapsed() > TEST_TIMEOUT {
            break false;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let _ = child.kill();
    let status = child.wait().unwrap();
    assert!(ready, "child missed {phase} boundary: {status}");
    assert_eq!(
        status.signal(),
        Some(9),
        "{phase} child must terminate by SIGKILL"
    );
}

#[cfg(unix)]
fn check_integrity(scratch: &Scratch) {
    let database = Connection::open(scratch.journal_path()).unwrap();
    let result: String = database
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(result, "ok");
}

#[cfg(unix)]
#[test]
fn sigkill_during_reconciliation_keeps_operation_unresolved() {
    let scratch = Scratch::new();
    let intent = intent();
    let source = Source::default();
    let rule = ReleaseRule(intent.clone());
    let missing = Provider::default();
    assert!(matches!(
        publisher(&source, &rule, &missing, &Evidence::VALID).execute(
            &mut scratch.journal(),
            &Grant::CURRENT,
            &intent,
            Recovery::LookupOnly
        ),
        Err(Error::ExternalUnavailable)
    ));

    kill_child_at_marker(&scratch, "confirming");
    let mut reopened = scratch.journal();
    check_integrity(&scratch);
    assert_eq!(
        reopened.state(&intent).unwrap(),
        Some(OperationState::Unresolved)
    );

    let settled = Provider {
        observation: Some(receipt(&intent)),
        ..Provider::default()
    };
    assert!(matches!(
        publisher(&source, &rule, &settled, &Evidence::VALID).execute(
            &mut reopened,
            &Grant::CURRENT,
            &intent,
            Recovery::LookupOnly
        ),
        Ok(OperationState::Confirmed(_))
    ));
    assert_eq!(settled.submits.load(SeqCst), 0);
}

#[test]
fn inconclusive_lookup_and_default_retry_issue_no_second_submit() {
    let scratch = Scratch::new();
    let intent = intent();
    let mut journal = scratch.journal();
    let provider = Provider::default();
    let source = Source::default();
    let rule = ReleaseRule(intent.clone());
    let caller = publisher(&source, &rule, &provider, &Evidence::VALID);
    assert!(matches!(
        caller.execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly),
        Err(Error::ExternalUnavailable)
    ));
    for mode in [Recovery::LookupOnly, Recovery::RetryIdempotently] {
        assert_eq!(
            caller
                .execute(&mut journal, &Grant::CURRENT, &intent, mode)
                .unwrap(),
            OperationState::Unresolved
        );
    }
    assert_eq!(provider.submits.load(SeqCst), 1);
    assert_eq!(provider.lookups.load(SeqCst), 2);
}

fn conflicting_revision() -> (Source, Intent) {
    let body = b"different authorized content for the same revision".to_vec();
    let binding = ArtifactRevision {
        digest: ContentDigest::of(&body),
        ..revision()
    };
    let proposed = Intent::new(binding.clone(), DESTINATION.into()).unwrap();
    (
        Source {
            revision: binding,
            release: Release::ExplicitlyPublic,
            body,
        },
        proposed,
    )
}

#[test]
fn confirmed_revision_rejects_conflicting_digest_without_second_submit() {
    let scratch = Scratch::new();
    let original = intent();
    let provider = Provider {
        submission: Some(receipt(&original)),
        ..Provider::default()
    };
    let mut journal = scratch.journal();
    let confirmed = publisher(
        &Source::default(),
        &ReleaseRule(original.clone()),
        &provider,
        &Evidence::VALID,
    )
    .execute(
        &mut journal,
        &Grant::CURRENT,
        &original,
        Recovery::LookupOnly,
    )
    .unwrap();
    assert!(matches!(confirmed, OperationState::Confirmed(_)));
    drop(journal);

    let (alternate, competing) = conflicting_revision();
    let mut reopened = scratch.journal();
    let result = publisher(
        &alternate,
        &ReleaseRule(competing.clone()),
        &provider,
        &Evidence::VALID,
    )
    .execute(
        &mut reopened,
        &Grant::CURRENT,
        &competing,
        Recovery::LookupOnly,
    );
    assert!(matches!(result, Err(Error::JournalConflict)));
    assert_eq!(reopened.state(&original).unwrap(), Some(confirmed));
    assert_eq!(reopened.state(&competing).unwrap(), None);
    assert_eq!(provider.submits.load(SeqCst), 1);
    assert_eq!(provider.lookups.load(SeqCst), 0);
}

struct RevocablePolicy {
    intent: Intent,
    approved: AtomicBool,
}

impl PublicationEligibility for RevocablePolicy {
    fn permits(&self, intent: &Intent) -> Result<bool, Error> {
        Ok(self.approved.load(SeqCst) && *intent == self.intent)
    }
}

#[test]
fn confirmed_receipt_does_not_bypass_revoked_public_release() {
    let scratch = Scratch::new();
    let intent = intent();
    let source = Source::default();
    let eligibility = RevocablePolicy {
        intent: intent.clone(),
        approved: AtomicBool::new(true),
    };
    let provider = Provider {
        submission: Some(receipt(&intent)),
        ..Provider::default()
    };
    let caller = Publisher {
        source: &source,
        eligibility: &eligibility,
        destination: &provider,
        evidence: &Evidence::VALID,
    };
    let mut journal = scratch.journal();
    let confirmed = caller
        .execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly)
        .unwrap();
    assert!(matches!(confirmed, OperationState::Confirmed(_)));
    eligibility.approved.store(false, SeqCst);
    assert!(matches!(
        caller.execute(
            &mut journal,
            &Grant::CURRENT,
            &intent,
            Recovery::RetryIdempotently
        ),
        Err(Error::Ineligible)
    ));
    assert_eq!(journal.state(&intent).unwrap(), Some(confirmed));
    assert_eq!(provider.submits.load(SeqCst), 1);
    assert_eq!(provider.lookups.load(SeqCst), 0);
}

#[test]
fn concurrent_identical_and_conflicting_intents_cannot_dispatch_twice() {
    let scratch = Scratch::new();
    let original = intent();
    let (entered, started) = sync_channel(1);
    let (release, resume) = sync_channel(1);
    let provider = Provider {
        submission: Some(receipt(&original)),
        gate: Some(DispatchGate {
            entered,
            resume: Mutex::new(resume),
        }),
        ..Provider::default()
    };
    let source = Source::default();
    let policy = ReleaseRule(original.clone());
    let caller = publisher(&source, &policy, &provider, &Evidence::VALID);
    let mut first_connection = scratch.journal();
    let mut other_connection = scratch.journal();

    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            caller.execute(
                &mut first_connection,
                &Grant::CURRENT,
                &original,
                Recovery::LookupOnly,
            )
        });
        started.recv_timeout(TEST_TIMEOUT).unwrap();
        let same = caller.execute(
            &mut other_connection,
            &Grant::CURRENT,
            &original,
            Recovery::LookupOnly,
        );
        let (alternate, competing) = conflicting_revision();
        let conflict = publisher(
            &alternate,
            &ReleaseRule(competing.clone()),
            &provider,
            &Evidence::VALID,
        )
        .execute(
            &mut other_connection,
            &Grant::CURRENT,
            &competing,
            Recovery::LookupOnly,
        );
        release.send(()).unwrap();

        assert_eq!(same.unwrap(), OperationState::Unresolved);
        assert!(matches!(conflict, Err(Error::JournalConflict)));
        let winner = first.join().unwrap().unwrap();
        assert!(matches!(winner, OperationState::Confirmed(_)));
        assert_eq!(other_connection.state(&original).unwrap(), Some(winner));
        assert_eq!(other_connection.state(&competing).unwrap(), None);
    });
    assert_eq!(provider.submits.load(SeqCst), 1);
    assert_eq!(provider.lookups.load(SeqCst), 1);
}

#[cfg(unix)]
#[test]
fn sigkill_after_dispatch_claim_and_confirmation_preserves_integrity() {
    for phase in ["claimed", "confirmed"] {
        let scratch = Scratch::new();
        kill_child_at_marker(&scratch, phase);
        let intent = intent();
        let mut journal = scratch.journal();
        check_integrity(&scratch);
        let state = journal.state(&intent).unwrap().unwrap();
        if phase == "claimed" {
            assert_eq!(state, OperationState::Unresolved);
        } else {
            let OperationState::Confirmed(receipt) = &state else {
                panic!("confirmation did not persist");
            };
            assert_eq!(receipt.remote_id(), "confirmed-provider-fixture");
        }
        let source = Source::default();
        let rule = ReleaseRule(intent.clone());
        let provider = Provider::default();
        assert_eq!(
            publisher(&source, &rule, &provider, &Evidence::VALID)
                .execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly)
                .unwrap(),
            state
        );
        assert_eq!(provider.submits.load(SeqCst), 0);
        assert_eq!(
            provider.lookups.load(SeqCst),
            usize::from(phase == "claimed")
        );
    }
}
