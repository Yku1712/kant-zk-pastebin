// SPDX-License-Identifier: Apache-2.0

use super::*;
use data_encoding::BASE32_NOPAD;
use rusqlite::Connection;
use std::{
    env,
    os::unix::process::ExitStatusExt,
    process::Stdio,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::SeqCst},
        mpsc::sync_channel,
    },
    thread,
    time::Duration,
};
use zos_int07_core::{ArtifactRevision, BodyKind, ContentDigest};
use zos_int08_authorized_artifact_bridge::{
    ArtifactMcpBridge, ArtifactRecord, AuthenticatedPrincipal, Classification,
    ContractCompatibleProvider, CredentialRecord, CredentialVerifier, Role,
};

const STORE_ID: &str = "int13-local-destination";
const CHUNK: usize = 256 * 1024;
const WAIT: Duration = Duration::from_secs(20);

/// Clock and lease fixture only. The real INT08 bridge separately validates its read and release
/// grants. This simulates a destination-scoped release decision with expiry and revocation.
struct TestReleaseAuthority {
    approved: Intent,
    now: AtomicU64,
    expires: u64,
    revoked: AtomicBool,
}

impl TestReleaseAuthority {
    fn new(intent: &Intent) -> Self {
        Self {
            approved: intent.clone(),
            now: AtomicU64::new(1),
            expires: 2,
            revoked: AtomicBool::new(false),
        }
    }
}

impl PublicationEligibility for TestReleaseAuthority {
    fn permits(&self, intent: &Intent) -> Result<bool, Error> {
        Ok(*intent == self.approved
            && self.now.load(SeqCst) < self.expires
            && !self.revoked.load(SeqCst))
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_owned();
        fs::create_dir(path.join("repo")).unwrap();
        fs::create_dir(path.join("stages")).unwrap();
        Self {
            _directory: directory,
            path,
        }
    }
}

fn observed<'a>(path: &Path, hook: &'a Observer<'a>) -> LocalDestination<'a> {
    LocalDestination::open_observed(
        STORE_ID,
        env::var_os("INT13_PROBE").expect("set INT13_PROBE to the corrected INT11 writer probe"),
        path.join("repo"),
        path.join("stages"),
        hook,
    )
    .unwrap()
}

fn bridge(path: &Path) -> Journal {
    Journal::open(path.join("bridge.sqlite")).unwrap()
}

fn body(tail: u8) -> Vec<u8> {
    (0..CHUNK + 31)
        .map(|i| if i < CHUNK { (i % 251) as u8 } else { tail })
        .collect()
}

fn intent(bytes: &[u8]) -> Intent {
    Intent::new(
        ArtifactRevision {
            source: "int08-contract-fixture".into(),
            tenant: "tenant-a".into(),
            artifact: "single-file".into(),
            revision: 3,
            digest: ContentDigest::of(bytes),
        },
        STORE_ID.into(),
    )
    .unwrap()
}

fn fixture_provider(intent: &Intent, bytes: Vec<u8>) -> ContractCompatibleProvider {
    ContractCompatibleProvider::empty().with_record(ArtifactRecord::new(
        intent.artifact().clone(),
        Classification::Public,
        BodyKind::SingleFile,
        bytes,
    ))
}

fn principal(tenant: &str) -> AuthenticatedPrincipal {
    CredentialVerifier::new([(
        "fixture-key",
        CredentialRecord::api_key(tenant, "fixture-user", Role::Collaborator),
    )])
    .authenticate("fixture-key")
    .unwrap()
}

fn receipt_rows(path: &Path) -> i64 {
    Connection::open(path.join("repo/completion.sqlite"))
        .unwrap()
        .query_row("SELECT count(*) FROM receipts", [], |row| row.get(0))
        .unwrap()
}

fn check_databases(path: &Path) {
    for relative in ["repo/completion.sqlite", "bridge.sqlite"] {
        let conn = Connection::open(path.join(relative)).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "integrity_check", |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }
}

fn flatfs_path(repo: &Path, cid: &Cid) -> PathBuf {
    let key = BASE32_NOPAD.encode(&cid.hash().to_bytes());
    repo.join("blocks")
        .join(&key[key.len() - 3..key.len() - 1])
        .join(format!("{key}.data"))
}

fn child(repo: &Path, root: &Cid) -> PathBuf {
    let root_file = flatfs_path(repo, root);
    fs::read_dir(repo.join("blocks"))
        .unwrap()
        .flat_map(|shard| fs::read_dir(shard.unwrap().path()).unwrap())
        .map(|file| file.unwrap().path())
        .find(|file| file.extension().is_some_and(|ext| ext == "data") && *file != root_file)
        .expect("authentic multiblock graph has a child")
}

#[test]
#[ignore = "requires corrected INT11 probe binary"]
fn unauthorized_expired_revoked_wrong_revision_or_wrong_digest_never_stage() {
    let f = Fixture::new();
    let bytes = body(11);
    let requested = intent(&bytes);
    let release = TestReleaseAuthority::new(&requested);
    let observer = |_, _: &Path, _: &Cid| -> Result<(), Error> {
        panic!("denied authorization must not run the producer");
    };
    let destination = observed(&f.path, &observer);
    let mut journal = bridge(&f.path);
    let source = ArtifactMcpBridge::new(fixture_provider(&requested, bytes.clone()));
    let user = principal("tenant-a");
    let other = principal("tenant-b");
    assert!(matches!(
        destination.execute(
            &mut journal,
            &source,
            &other,
            &release,
            &requested,
            Recovery::LookupOnly
        ),
        Err(Error::AccessDenied)
    ));

    release.now.store(release.expires, SeqCst);
    assert!(matches!(
        destination.execute(
            &mut journal,
            &source,
            &user,
            &release,
            &requested,
            Recovery::LookupOnly
        ),
        Err(Error::Ineligible)
    ));
    release.now.store(1, SeqCst);
    release.revoked.store(true, SeqCst);
    assert!(matches!(
        destination.execute(
            &mut journal,
            &source,
            &user,
            &release,
            &requested,
            Recovery::LookupOnly
        ),
        Err(Error::Ineligible)
    ));
    release.revoked.store(false, SeqCst);

    let mut revoked_read = fixture_provider(&requested, bytes.clone());
    revoked_read.set_revoked(true);
    assert!(matches!(
        destination.execute(
            &mut journal,
            &ArtifactMcpBridge::new(revoked_read),
            &user,
            &release,
            &requested,
            Recovery::LookupOnly
        ),
        Err(Error::AccessDenied)
    ));
    let wrong_rev = Intent::new(
        ArtifactRevision {
            revision: 2,
            ..requested.artifact().clone()
        },
        STORE_ID.into(),
    )
    .unwrap();
    assert!(matches!(
        destination.execute(
            &mut journal,
            &source,
            &user,
            &TestReleaseAuthority::new(&wrong_rev),
            &wrong_rev,
            Recovery::LookupOnly
        ),
        Err(Error::SourceUnavailable)
    ));
    let wrong_digest = intent(&body(12));
    assert!(matches!(
        destination.execute(
            &mut journal,
            &source,
            &user,
            &TestReleaseAuthority::new(&wrong_digest),
            &wrong_digest,
            Recovery::LookupOnly
        ),
        Err(Error::InvalidBinding)
    ));
    let mut substituted = fixture_provider(&requested, bytes);
    substituted.set_substitute_revision(Some(5));
    assert!(matches!(
        destination.execute(
            &mut journal,
            &ArtifactMcpBridge::new(substituted),
            &user,
            &release,
            &requested,
            Recovery::LookupOnly
        ),
        Err(Error::InvalidBinding)
    ));

    assert_eq!(journal.state(&requested).unwrap(), None);
    assert_eq!(journal.state(&wrong_rev).unwrap(), None);
    assert_eq!(journal.state(&wrong_digest).unwrap(), None);
    assert_eq!(receipt_rows(&f.path), 0);
    assert_eq!(fs::read_dir(f.path.join("stages")).unwrap().count(), 0);
}

#[test]
#[ignore = "requires corrected INT11 probe binary"]
fn authentic_staged_child_mutations_never_confirm() {
    for missing in [false, true] {
        let f = Fixture::new();
        let bytes = body(13);
        let intent = intent(&bytes);
        let source = ArtifactMcpBridge::new(fixture_provider(&intent, bytes));
        let observer = |phase, path: &Path, root: &Cid| {
            if phase == Phase::Staged {
                let path = child(path, root);
                if missing {
                    fs::remove_file(path).unwrap();
                } else {
                    let mut corrupted = fs::read(&path).unwrap();
                    corrupted[0] ^= 1;
                    fs::write(path, corrupted).unwrap();
                }
            }
            Ok(())
        };
        let destination = observed(&f.path, &observer);
        let mut journal = bridge(&f.path);
        let result = destination.execute(
            &mut journal,
            &source,
            &principal("tenant-a"),
            &TestReleaseAuthority::new(&intent),
            &intent,
            Recovery::LookupOnly,
        );
        if missing {
            assert!(matches!(result, Err(Error::ExternalUnavailable)));
        } else {
            assert!(matches!(result, Err(Error::InvalidReceipt)));
        }
        assert_eq!(
            journal.state(&intent).unwrap(),
            Some(OperationState::Unresolved)
        );
        assert!(destination.lookup(&intent).unwrap().is_none());
        assert_eq!(receipt_rows(&f.path), 0);
        println!(
            "authentic staged child {}: journal unresolved, provider receipt absent",
            if missing { "missing" } else { "altered" }
        );
    }
}

#[test]
#[ignore = "subprocess entry point for the integration crash cases"]
fn crash_child() {
    let Some(path) = env::var_os("INT13_CRASH_PATH").map(PathBuf::from) else {
        return;
    };
    let boundary = env::var("INT13_CRASH_BOUNDARY").unwrap();
    let bytes = body(21);
    let intent = intent(&bytes);
    let source = ArtifactMcpBridge::new(fixture_provider(&intent, bytes));
    let observer = |phase, _: &Path, root: &Cid| {
        if (phase == Phase::BeforeChildRead && boundary == "before")
            || (phase == Phase::ProviderCommitted && boundary == "after")
        {
            let temporary = path.join("marker.tmp");
            fs::write(&temporary, root.to_string()).unwrap();
            fs::rename(temporary, path.join("ready")).unwrap();
            loop {
                thread::park();
            }
        }
        Ok(())
    };
    let destination = observed(&path, &observer);
    let result = destination.execute(
        &mut bridge(&path),
        &source,
        &principal("tenant-a"),
        &TestReleaseAuthority::new(&intent),
        &intent,
        Recovery::LookupOnly,
    );
    panic!("child never parked at {boundary}: {result:?}");
}

#[test]
#[ignore = "requires corrected INT11 probe binary"]
fn sigkill_before_and_after_destination_commit_reconciles_without_duplicate_receipts() {
    for boundary in ["before", "after"] {
        let f = Fixture::new();
        let bytes = body(21);
        let intent = intent(&bytes);
        let source = ArtifactMcpBridge::new(fixture_provider(&intent, bytes));
        let user = principal("tenant-a");
        let release = TestReleaseAuthority::new(&intent);

        let normal = observed(&f.path, &|_, _, _| Ok(()));
        let _ = bridge(&f.path);
        let mut child_process = Command::new(env::current_exe().unwrap())
            .args(["--ignored", "--exact", "tests::crash_child"])
            .env("INT13_CRASH_PATH", &f.path)
            .env("INT13_CRASH_BOUNDARY", boundary)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let marker = loop {
            if let Ok(root) = fs::read_to_string(f.path.join("ready")) {
                break Some(root);
            }
            if child_process.try_wait().unwrap().is_some() || started.elapsed() > WAIT {
                break None;
            }
            thread::sleep(Duration::from_millis(10));
        };
        let before_kill = marker.as_ref().map(|_| normal.lookup(&intent));
        let _ = child_process.kill();
        let status = child_process.wait().unwrap();
        let root: Cid = marker
            .expect("child must reach phase boundary")
            .parse()
            .unwrap();
        assert_eq!(status.signal(), Some(9));
        assert!(
            flatfs_path(&f.path.join("repo"), &root).is_file(),
            "root exists physically at both points"
        );
        assert_eq!(before_kill.unwrap().unwrap().is_some(), boundary == "after");
        drop(normal);

        let stage_count = AtomicUsize::new(0);
        let count = |phase, _: &Path, _: &Cid| {
            if phase == Phase::Staged {
                stage_count.fetch_add(1, SeqCst);
            }
            Ok(())
        };
        let reopened = observed(&f.path, &count);
        let mut journal = bridge(&f.path);
        assert_eq!(
            journal.state(&intent).unwrap(),
            Some(OperationState::Unresolved)
        );
        assert_eq!(
            reopened.lookup(&intent).unwrap().is_some(),
            boundary == "after"
        );
        check_databases(&f.path);
        assert_eq!(fs::read_dir(f.path.join("stages")).unwrap().count(), 1);
        let orphans = reopened.store.orphans().unwrap();
        if boundary == "before" {
            assert_eq!(
                orphans.unreferenced_blocks,
                [flatfs_path(&f.path.join("repo"), &root)]
            );
        } else {
            assert!(orphans.unreferenced_blocks.is_empty());
        }
        let lookup = reopened
            .execute(
                &mut journal,
                &source,
                &user,
                &release,
                &intent,
                Recovery::LookupOnly,
            )
            .unwrap();
        if boundary == "before" {
            assert_eq!(lookup, OperationState::Unresolved);
            assert_eq!(stage_count.load(SeqCst), 0);
        } else {
            assert!(matches!(lookup, OperationState::Confirmed(_)));
        }
        let recovered = reopened
            .execute(
                &mut journal,
                &source,
                &user,
                &release,
                &intent,
                Recovery::RetryIdempotently,
            )
            .unwrap();
        let OperationState::Confirmed(receipt) = &recovered else {
            panic!("recovery must confirm")
        };
        assert_eq!(receipt.ipfs().unwrap().cid(), &root);
        assert_eq!(receipt_rows(&f.path), 1);
        assert_eq!(stage_count.load(SeqCst), usize::from(boundary == "before"));
        assert_eq!(journal.state(&intent).unwrap(), Some(recovered));
        assert!(
            reopened
                .store
                .orphans()
                .unwrap()
                .unreferenced_blocks
                .is_empty()
        );
        assert_eq!(
            fs::read_dir(f.path.join("stages")).unwrap().count(),
            1,
            "crashed staging requires operator cleanup"
        );
        println!(
            "SIGKILL {boundary} destination COMMIT: bridge was unresolved; recovered one receipt, stages={}",
            stage_count.load(SeqCst)
        );
    }
}

#[test]
#[ignore = "requires corrected INT11 probe binary"]
fn lost_ack_with_valid_local_receipt_is_reconciled_read_only() {
    let f = Fixture::new();
    let bytes = body(23);
    let intent = intent(&bytes);
    let source = ArtifactMcpBridge::new(fixture_provider(&intent, bytes));
    let release = TestReleaseAuthority::new(&intent);
    let user = principal("tenant-a");
    let observer = |phase, _: &Path, _: &Cid| {
        if phase == Phase::ProviderCommitted {
            return Err(Error::ExternalUnavailable);
        }
        Ok(())
    };
    let destination = observed(&f.path, &observer);
    assert!(matches!(
        destination.execute(
            &mut bridge(&f.path),
            &source,
            &user,
            &release,
            &intent,
            Recovery::LookupOnly
        ),
        Err(Error::ExternalUnavailable)
    ));
    assert!(destination.lookup(&intent).unwrap().is_some());
    assert_eq!(
        bridge(&f.path).state(&intent).unwrap(),
        Some(OperationState::Unresolved)
    );
    let reopened = observed(&f.path, &|_, _, _| {
        panic!("lost acknowledgement must be reconciled without running the writer")
    });
    let mut journal = bridge(&f.path);
    let result = reopened
        .execute(
            &mut journal,
            &source,
            &user,
            &release,
            &intent,
            Recovery::LookupOnly,
        )
        .unwrap();
    assert!(matches!(result, OperationState::Confirmed(_)));
    assert_eq!(receipt_rows(&f.path), 1);
    assert_eq!(journal.state(&intent).unwrap(), Some(result));
}

#[test]
#[ignore = "requires corrected INT11 probe binary"]
fn concurrent_identical_retry_is_idempotent_and_conflicting_revision_is_denied() {
    let f = Fixture::new();
    let bytes = body(24);
    let request = intent(&bytes);
    let source = ArtifactMcpBridge::new(fixture_provider(&request, bytes));
    let release = TestReleaseAuthority::new(&request);
    let user = principal("tenant-a");
    let (entered, started) = sync_channel(1);
    let (resume, release_first) = sync_channel(1);
    let release_first = Mutex::new(release_first);
    let staged = AtomicUsize::new(0);
    let observer = |phase, _: &Path, _: &Cid| {
        if phase == Phase::Staged && staged.fetch_add(1, SeqCst) == 0 {
            entered.send(()).map_err(|_| Error::ExternalUnavailable)?;
            release_first
                .lock()
                .unwrap()
                .recv_timeout(WAIT)
                .map_err(|_| Error::ExternalUnavailable)?;
        }
        Ok(())
    };
    let destination = observed(&f.path, &observer);
    let mut first = bridge(&f.path);
    let mut second = bridge(&f.path);
    thread::scope(|scope| {
        let pending = scope.spawn(|| {
            destination.execute(
                &mut first,
                &source,
                &user,
                &release,
                &request,
                Recovery::LookupOnly,
            )
        });
        started.recv_timeout(WAIT).unwrap();
        let in_flight = destination.execute(
            &mut second,
            &source,
            &user,
            &release,
            &request,
            Recovery::LookupOnly,
        );
        let retried = destination.execute(
            &mut second,
            &source,
            &user,
            &release,
            &request,
            Recovery::RetryIdempotently,
        );
        resume.send(()).unwrap();
        assert_eq!(in_flight.unwrap(), OperationState::Unresolved);
        let confirmed = pending.join().unwrap().unwrap();
        assert!(matches!(confirmed, OperationState::Confirmed(_)));
        assert_eq!(confirmed, retried.unwrap());
    });
    assert_eq!(
        staged.load(SeqCst),
        2,
        "one initial stage and one explicitly requested retry"
    );
    assert_eq!(receipt_rows(&f.path), 1);
    let conflict_body = body(25);
    let conflict = intent(&conflict_body);
    let conflict_source = ArtifactMcpBridge::new(fixture_provider(&conflict, conflict_body));
    assert!(matches!(
        destination.execute(
            &mut second,
            &conflict_source,
            &user,
            &TestReleaseAuthority::new(&conflict),
            &conflict,
            Recovery::RetryIdempotently
        ),
        Err(Error::JournalConflict)
    ));
    assert_eq!(second.state(&conflict).unwrap(), None);
    assert_eq!(staged.load(SeqCst), 2);
    assert_eq!(receipt_rows(&f.path), 1);
    check_databases(&f.path);
}

#[test]
#[ignore = "requires corrected INT11 probe binary"]
fn another_readback_detects_corruption_after_confirmation() {
    let f = Fixture::new();
    let bytes = body(27);
    let intent = intent(&bytes);
    let source = ArtifactMcpBridge::new(fixture_provider(&intent, bytes));
    let release = TestReleaseAuthority::new(&intent);
    let user = principal("tenant-a");
    let staged = AtomicUsize::new(0);
    let observer = |phase, _: &Path, _: &Cid| {
        if phase == Phase::Staged {
            staged.fetch_add(1, SeqCst);
        }
        Ok(())
    };
    let destination = observed(&f.path, &observer);
    let mut journal = bridge(&f.path);
    let state = destination
        .execute(
            &mut journal,
            &source,
            &user,
            &release,
            &intent,
            Recovery::LookupOnly,
        )
        .unwrap();
    let OperationState::Confirmed(receipt) = &state else {
        panic!("fixture must first confirm")
    };
    let damaged = child(&f.path.join("repo"), receipt.ipfs().unwrap().cid());
    let mut corrupt = fs::read(&damaged).unwrap();
    corrupt[0] ^= 1;
    fs::write(damaged, corrupt).unwrap();
    assert!(matches!(
        destination.lookup(&intent),
        Err(Error::InvalidReceipt)
    ));
    assert!(matches!(
        destination.execute(
            &mut journal,
            &source,
            &user,
            &release,
            &intent,
            Recovery::RetryIdempotently
        ),
        Err(Error::InvalidReceipt)
    ));
    assert_eq!(
        journal.state(&intent).unwrap(),
        Some(state),
        "historical receipt persists, current readback fails"
    );
    assert_eq!(staged.load(SeqCst), 1);
    assert_eq!(receipt_rows(&f.path), 1);
}

#[test]
#[ignore = "requires corrected INT11 probe binary"]
fn revocation_or_expiry_during_promotion_blocks_bridge_confirmation() {
    for expiry in [false, true] {
        let f = Fixture::new();
        let bytes = body(41);
        let intent = intent(&bytes);
        let source = ArtifactMcpBridge::new(fixture_provider(&intent, bytes));
        let release = TestReleaseAuthority::new(&intent);
        let changed = AtomicBool::new(false);
        let observer = |phase, _: &Path, _: &Cid| {
            if phase == Phase::BeforeChildRead && !changed.swap(true, SeqCst) {
                assert!(release.permits(&intent).unwrap());
                if expiry {
                    release.now.store(release.expires, SeqCst);
                } else {
                    release.revoked.store(true, SeqCst);
                }
            }
            Ok(())
        };
        let destination = observed(&f.path, &observer);
        let mut journal = bridge(&f.path);
        let user = principal("tenant-a");
        assert!(matches!(
            destination.execute(
                &mut journal,
                &source,
                &user,
                &release,
                &intent,
                Recovery::LookupOnly
            ),
            Err(Error::Ineligible)
        ));
        assert!(changed.load(SeqCst));
        assert_eq!(
            journal.state(&intent).unwrap(),
            Some(OperationState::Unresolved)
        );
        assert!(
            destination.lookup(&intent).unwrap().is_some(),
            "already submitted work may commit locally"
        );
        assert!(matches!(
            destination.execute(
                &mut journal,
                &source,
                &user,
                &release,
                &intent,
                Recovery::RetryIdempotently
            ),
            Err(Error::Ineligible)
        ));
        assert_eq!(receipt_rows(&f.path), 1);
        println!(
            "during promotion: {} withheld bridge confirmation; one local receipt retained",
            if expiry { "expiry" } else { "revocation" }
        );
    }
}

#[test]
#[ignore = "requires corrected INT11 probe binary"]
fn oversized_authorized_snapshot_is_rejected_before_staging_or_receipt() {
    let f = Fixture::new();
    let bytes = vec![0x41; MAX_PUBLICATION_BYTES + 1];
    let intent = intent(&bytes);
    let source = ArtifactMcpBridge::new(fixture_provider(&intent, bytes));
    let destination = observed(&f.path, &|_, _, _| {
        panic!("oversized input must not reach the writer")
    });
    let mut journal = bridge(&f.path);
    assert!(matches!(
        destination.execute(
            &mut journal,
            &source,
            &principal("tenant-a"),
            &TestReleaseAuthority::new(&intent),
            &intent,
            Recovery::LookupOnly
        ),
        Err(Error::ResourceLimit)
    ));
    assert_eq!(journal.state(&intent).unwrap(), None);
    assert_eq!(receipt_rows(&f.path), 0);
    assert_eq!(fs::read_dir(f.path.join("stages")).unwrap().count(), 0);
}
