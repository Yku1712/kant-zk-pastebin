// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    fs,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::Instant,
};

fn intent() -> Intent {
    Intent::new(
        ArtifactRevision {
            source: "source".into(),
            tenant: "tenant".into(),
            artifact: "artifact".into(),
            revision: u64::MAX,
            digest: ContentDigest::of(b"journal fixture"),
        },
        "destination".into(),
    )
    .unwrap()
}

fn receipt() -> ConfirmedReceipt {
    ConfirmedReceipt {
        remote_id: "fixture".into(),
        ipfs: None,
    }
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("zos-int07-crash-{}-{suffix}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test directory: {error}"),
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn operation_id_known_vector_and_scope() {
    let intent = intent();
    assert_eq!(
        intent.operation().to_string(),
        "cf85121d9880f6ece1cdf25221fd209f1d9a403d3a4c32d4f5504799882ce245"
    );
    let different_digest = ArtifactRevision {
        digest: ContentDigest::of(b"different fixture"),
        ..intent.artifact().clone()
    };
    let conflicting = Intent::new(different_digest, intent.destination().into()).unwrap();
    let other_destination = Intent::new(intent.artifact().clone(), "different".into()).unwrap();
    assert_ne!(intent.operation(), conflicting.operation());
    assert_ne!(intent.operation(), other_destination.operation());

    let scratch = Scratch::new();
    let mut journal = Journal::open(scratch.0.join("journal.sqlite")).unwrap();
    journal.prepare(&intent).unwrap();
    assert!(matches!(
        journal.prepare(&conflicting),
        Err(Error::JournalConflict)
    ));
    journal.prepare(&other_destination).unwrap();
    assert_eq!(journal.pending().unwrap().len(), 2);
}

#[test]
fn confirmed_receipt_is_write_once() {
    let scratch = Scratch::new();
    let mut journal = Journal::open(scratch.0.join("journal.sqlite")).unwrap();
    let intent = intent();
    journal.prepare(&intent).unwrap();
    assert!(matches!(
        journal.claim_or_read(&intent).unwrap(),
        Claim::FirstDispatch
    ));
    journal.confirm(&intent, &receipt()).unwrap();
    journal.confirm(&intent, &receipt()).unwrap();
    let conflicting = ConfirmedReceipt {
        remote_id: "other".into(),
        ipfs: None,
    };
    assert!(matches!(
        journal.confirm(&intent, &conflicting),
        Err(Error::JournalConflict)
    ));
    assert_eq!(
        journal.state(&intent).unwrap(),
        Some(OperationState::Confirmed(receipt()))
    );
}

#[test]
#[ignore = "invoked by the crash test in a disposable child process"]
fn crash_worker() {
    let directory = std::env::var_os("ZOS_INT07_CRASH_DIR").unwrap();
    let directory = Path::new(&directory);
    let phase = std::env::var("ZOS_INT07_CRASH_PHASE").unwrap();
    let mut journal = Journal::open(directory.join("journal.sqlite")).unwrap();
    let intent = intent();
    match phase.as_str() {
        "prepare-uncommitted" => {
            let tx = journal
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute(
                "INSERT INTO publication
                    (operation, source, tenant, artifact, revision, digest, destination, state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'PREPARED')",
                params![
                    intent.operation.0,
                    intent.artifact.source,
                    intent.artifact.tenant,
                    intent.artifact.artifact,
                    intent.artifact.revision.to_be_bytes(),
                    intent.artifact.digest.0,
                    intent.destination,
                ],
            )
            .unwrap();
            signal_and_park(directory);
        }
        "prepared" => journal.prepare(&intent).unwrap(),
        "claim-uncommitted" | "confirm-uncommitted" => {
            journal.prepare(&intent).unwrap();
            let confirming = phase == "confirm-uncommitted";
            if confirming {
                journal.claim_or_read(&intent).unwrap();
            }
            let tx = journal
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            if confirming {
                tx.execute(
                    "UPDATE publication SET state = 'CONFIRMED', remote_id = ?2 WHERE operation = ?1",
                    params![intent.operation.0, receipt().remote_id],
                ).unwrap();
            } else {
                tx.execute(
                    "UPDATE publication SET state = 'UNRESOLVED' WHERE operation = ?1",
                    [intent.operation.0],
                )
                .unwrap();
            }
            signal_and_park(directory);
        }
        "claimed" | "confirmed" => {
            journal.prepare(&intent).unwrap();
            journal.claim_or_read(&intent).unwrap();
            if phase == "confirmed" {
                journal.confirm(&intent, &receipt()).unwrap();
            }
        }
        _ => panic!("unknown crash test phase"),
    }
    signal_and_park(directory);
}

fn signal_and_park(directory: &Path) -> ! {
    fs::write(directory.join("ready"), b"ready").unwrap();
    loop {
        thread::park();
    }
}

#[test]
fn process_kill_before_and_after_each_commit() {
    for (phase, expected) in [
        ("prepare-uncommitted", None),
        ("prepared", Some(OperationState::Prepared)),
        ("claim-uncommitted", Some(OperationState::Prepared)),
        ("claimed", Some(OperationState::Unresolved)),
        ("confirm-uncommitted", Some(OperationState::Unresolved)),
        ("confirmed", Some(OperationState::Confirmed(receipt()))),
    ] {
        let scratch = Scratch::new();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "journal::tests::crash_worker"])
            .env("ZOS_INT07_CRASH_DIR", &scratch.0)
            .env("ZOS_INT07_CRASH_PHASE", phase)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        let ready = loop {
            if scratch.0.join("ready").exists() {
                break true;
            }
            if child.try_wait().unwrap().is_some() || start.elapsed() > Duration::from_secs(15) {
                break false;
            }
            thread::sleep(Duration::from_millis(10));
        };
        let killed = child.kill();
        let status = child.wait().unwrap();
        assert!(ready, "worker did not reach {phase}: {status}");
        killed.unwrap();
        assert!(!status.success(), "{phase}");

        let intent = intent();
        let mut journal = Journal::open(scratch.0.join("journal.sqlite")).unwrap();
        assert_eq!(journal.state(&intent).unwrap(), expected, "{phase}");
        if matches!(expected, None | Some(OperationState::Prepared)) {
            journal.prepare(&intent).unwrap();
            assert!(
                matches!(
                    journal.claim_or_read(&intent).unwrap(),
                    Claim::FirstDispatch
                ),
                "{phase}"
            );
        } else {
            assert!(
                matches!(journal.claim_or_read(&intent).unwrap(), Claim::Existing(_)),
                "{phase}"
            );
        }
    }
}
