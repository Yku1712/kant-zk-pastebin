// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    env,
    os::unix::{fs::MetadataExt, process::ExitStatusExt},
    process::{Command, Stdio},
    sync::Barrier,
    thread,
};
use tempfile::TempDir;
use zos_int07_core::ArtifactRevision;

const DESTINATION: &str = "durability-fixture";
const CHUNK: usize = 256 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);

struct Candidate {
    path: PathBuf,
    input: PathBuf,
    root: Cid,
}

fn make_candidate(temp: &TempDir, name: &str, content: &[u8], probe_var: &str) -> Candidate {
    let path = temp.path().join(name);
    let input = temp.path().join(format!("{name}.input"));
    fs::create_dir(&path).unwrap();
    fs::write(&input, content).unwrap();
    let probe =
        env::var_os(probe_var).unwrap_or_else(|| panic!("set {probe_var} to an INT11 probe"));
    let result = Command::new(probe)
        .env("IPFS_PATH", &path)
        .arg("add")
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = std::str::from_utf8(&result.stdout).unwrap().trim();
    let root: Cid = text.parse().unwrap();
    assert_eq!(root.codec(), 0x70);
    assert_eq!(root.to_string(), text);
    Candidate { path, input, root }
}

fn content(length: usize, tail: u8) -> Vec<u8> {
    (0..length)
        .map(|i| if i < CHUNK { (i % 251) as u8 } else { tail })
        .collect()
}

fn intent(artifact: &str, bytes: &[u8]) -> Intent {
    Intent::new(
        ArtifactRevision {
            source: "authentic-int11-probe".into(),
            tenant: "isolated-tenant".into(),
            artifact: artifact.into(),
            revision: 1,
            digest: ContentDigest::of(bytes),
        },
        DESTINATION.into(),
    )
    .unwrap()
}

fn separate_store(temp: &TempDir, name: &str) -> Store {
    let directory = temp.path().join(name);
    fs::create_dir(&directory).unwrap();
    Store::open(directory, DESTINATION).unwrap()
}

fn block_path(repo: &Path, cid: &Cid) -> PathBuf {
    let key = BASE32_NOPAD.encode(&cid.hash().to_bytes());
    repo.join("blocks")
        .join(&key[key.len() - 3..key.len() - 1])
        .join(format!("{key}.data"))
}

fn check_database(store: &Store) {
    let database = store.connection().unwrap();
    assert_eq!(
        database
            .pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        database
            .pragma_query_value(None, "integrity_check", |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    let violations: i64 = database
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
}

#[test]
#[ignore = "requires the locked INT11 patched writer probe"]
fn injected_io_failures_gate_receipts_before_and_after_commit() {
    let temp = tempfile::tempdir().unwrap();
    let payload = content(CHUNK + 27, 4);
    let intent = intent("faults", &payload);
    let candidate = make_candidate(&temp, "staging", &payload, "INT12_PROBE");
    let staging = Flatfs::open(&candidate.path).unwrap();
    for phase in [
        Phase::BeforeBlock,
        Phase::DuringTempWrite,
        Phase::AfterFileSync,
        Phase::AfterLink,
        Phase::AfterBlockSync,
        Phase::BeforeCommit,
        Phase::AfterCommit,
    ] {
        let store = separate_store(&temp, &format!("{phase:?}"));
        let result = store.commit_observed(
            &intent,
            &candidate.root,
            payload.len(),
            &staging,
            |current, _| {
                if current == phase {
                    return Err(io::Error::other("injected I/O fault"));
                }
                Ok(())
            },
        );
        if matches!(phase, Phase::BeforeCommit | Phase::AfterCommit) {
            assert!(
                matches!(result, Err(Error::Io(error)) if error.to_string() == "injected I/O fault")
            );
        } else {
            assert!(
                matches!(result, Err(Error::Verification(VerifyError::Io(error))) if error.to_string() == "injected I/O fault")
            );
        }
        let recovered = store.lookup(&intent).unwrap();
        assert_eq!(
            recovered.is_some(),
            phase == Phase::AfterCommit,
            "{phase:?}"
        );
        let inventory = store.orphans().unwrap();
        assert_eq!(
            inventory.abandoned_temporary_files.len(),
            0,
            "normal unwinding cleans temporary files"
        );
        let expected_orphans = match phase {
            Phase::AfterLink | Phase::AfterBlockSync => 1,
            Phase::BeforeCommit => 3,
            _ => 0,
        };
        assert_eq!(
            inventory.unreferenced_blocks.len(),
            expected_orphans,
            "{phase:?}"
        );
        let receipt = store
            .commit(&intent, &candidate.root, payload.len(), &staging)
            .unwrap();
        assert_eq!(store.lookup(&intent).unwrap(), Some(receipt));
        assert!(store.orphans().unwrap().unreferenced_blocks.is_empty());
        check_database(&store);
        println!(
            "injected {phase:?}: receipt before replay={}, orphans={expected_orphans}",
            recovered.is_some()
        );
    }
}

#[test]
#[ignore = "subprocess entry point for process-kill recovery tests"]
fn crash_child() {
    let Some(path) = env::var_os("INT12_CRASH_STORE").map(PathBuf::from) else {
        return;
    };
    let stage = PathBuf::from(env::var_os("INT12_CRASH_STAGE").unwrap());
    let input = PathBuf::from(env::var_os("INT12_CRASH_INPUT").unwrap());
    let root: Cid = env::var("INT12_CRASH_CID").unwrap().parse().unwrap();
    let phase = env::var("INT12_CRASH_PHASE").unwrap();
    let payload = fs::read(input).unwrap();
    let store = Store::open(&path, DESTINATION).unwrap();
    let result = store.commit_observed(
        &intent("crash", &payload),
        &root,
        payload.len(),
        &Flatfs::open(stage).unwrap(),
        |current, _| {
            if format!("{current:?}") == phase {
                fs::write(path.join("ready"), b"phase reached")?;
                loop {
                    thread::park();
                }
            }
            Ok(())
        },
    );
    panic!("failed to park at {phase}: {result:?}");
}

fn kill_at_phase(store: &Store, intent: &Intent, candidate: &Candidate, phase: Phase) {
    let mut child = Command::new(env::current_exe().unwrap())
        .args(["--ignored", "--exact", "tests::crash_child"])
        .env("INT12_CRASH_STORE", &store.path)
        .env("INT12_CRASH_STAGE", &candidate.path)
        .env("INT12_CRASH_INPUT", &candidate.input)
        .env("INT12_CRASH_CID", candidate.root.to_string())
        .env("INT12_CRASH_PHASE", format!("{phase:?}"))
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let arrived = loop {
        if store.path.join("ready").exists() {
            break true;
        }
        if child.try_wait().unwrap().is_some() || start.elapsed() > TIMEOUT {
            break false;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let visible_while_paused = arrived.then(|| store.lookup(intent));
    let _ = child.kill();
    let status = child.wait().unwrap();
    assert!(arrived, "child never reached {phase:?}: {status}");
    assert_eq!(status.signal(), Some(9));
    assert_eq!(
        visible_while_paused.unwrap().unwrap().is_some(),
        phase == Phase::AfterCommit
    );
}

#[test]
#[ignore = "requires the locked INT11 patched writer probe"]
fn real_sigkill_during_persistence_and_around_commit() {
    let temp = tempfile::tempdir().unwrap();
    let payload = content(CHUNK + 19, 25);
    let intent = intent("crash", &payload);
    let candidate = make_candidate(&temp, "staging", &payload, "INT12_PROBE");
    let staging = Flatfs::open(&candidate.path).unwrap();

    for phase in [
        Phase::BeforeBlock,
        Phase::DuringTempWrite,
        Phase::AfterFileSync,
        Phase::AfterLink,
        Phase::AfterBlockSync,
        Phase::BeforeCommit,
        Phase::AfterCommit,
    ] {
        let store = separate_store(&temp, &format!("{phase:?}"));
        kill_at_phase(&store, &intent, &candidate, phase);
        let reopened = Store::open(&store.path, DESTINATION).unwrap();
        check_database(&reopened);
        let recovered = reopened.lookup(&intent).unwrap();
        assert_eq!(
            recovered.is_some(),
            phase == Phase::AfterCommit,
            "{phase:?}"
        );
        let orphans = reopened.orphans().unwrap();
        let root_block = block_path(&reopened.path, &candidate.root);
        let expected_temp = usize::from(matches!(
            phase,
            Phase::DuringTempWrite | Phase::AfterFileSync | Phase::AfterLink
        ));
        let expected_orphans = match phase {
            Phase::AfterLink | Phase::AfterBlockSync => 1,
            Phase::BeforeCommit => 3,
            _ => 0,
        };
        assert_eq!(
            orphans.abandoned_temporary_files.len(),
            expected_temp,
            "{phase:?}"
        );
        assert_eq!(
            orphans.unreferenced_blocks.len(),
            expected_orphans,
            "{phase:?}"
        );
        let root_size = fs::metadata(block_path(&candidate.path, &candidate.root))
            .unwrap()
            .len();
        if let [temp] = orphans.abandoned_temporary_files.as_slice() {
            let expected_size = if phase == Phase::DuringTempWrite {
                root_size / 2
            } else {
                root_size
            };
            assert_eq!(fs::metadata(temp).unwrap().len(), expected_size);
        }
        assert_eq!(
            root_block.is_file(),
            matches!(
                phase,
                Phase::AfterLink | Phase::AfterBlockSync | Phase::BeforeCommit | Phase::AfterCommit
            )
        );
        let before_retry = fs::metadata(&root_block).ok().map(|entry| entry.ino());
        let receipt = reopened
            .commit(&intent, &candidate.root, payload.len(), &staging)
            .unwrap();
        if let Some(inode) = before_retry {
            assert_eq!(fs::metadata(&root_block).unwrap().ino(), inode);
        }
        assert_eq!(reopened.lookup(&intent).unwrap(), Some(receipt));
        assert!(reopened.orphans().unwrap().unreferenced_blocks.is_empty());
        println!(
            "SIGKILL {phase:?}: receipt={}, unreferenced blocks={expected_orphans}, temporary files={expected_temp}",
            recovered.is_some()
        );
    }
}

#[test]
#[ignore = "requires the locked INT11 patched writer probe"]
fn overlapping_identical_and_different_commits_are_immutable() {
    let temp = tempfile::tempdir().unwrap();
    let a = content(CHUNK + 17, 10);
    let b = content(CHUNK + 17, 20);
    let a_candidate = make_candidate(&temp, "a", &a, "INT12_PROBE");
    let b_candidate = make_candidate(&temp, "b", &b, "INT12_PROBE");
    let store = separate_store(&temp, "target");
    let a_intent = intent("a", &a);
    let b_intent = intent("b", &b);
    let start = Barrier::new(3);
    let receipts = thread::scope(|scope| {
        let tasks = [
            (&a_intent, &a_candidate, a.len()),
            (&a_intent, &a_candidate, a.len()),
            (&b_intent, &b_candidate, b.len()),
        ];
        let mut workers = Vec::new();
        for (intent, candidate, length) in tasks {
            let store = &store;
            let start = &start;
            workers.push(scope.spawn(move || {
                start.wait();
                let local = Store::open(&store.path, DESTINATION).unwrap();
                local
                    .commit(
                        intent,
                        &candidate.root,
                        length,
                        &Flatfs::open(&candidate.path).unwrap(),
                    )
                    .unwrap()
            }));
        }
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(receipts[0], receipts[1]);
    assert_ne!(receipts[0], receipts[2]);
    assert_eq!(
        store.lookup(&a_intent).unwrap().as_ref(),
        Some(&receipts[0])
    );
    assert_eq!(
        store.lookup(&b_intent).unwrap().as_ref(),
        Some(&receipts[2])
    );
    let database = store.connection().unwrap();
    let unique: i64 = database
        .query_row("SELECT count(DISTINCT key) FROM receipt_blocks", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(unique, 5, "two roots and two tails share one first leaf");
    let competing = intent("a", &b);
    assert!(matches!(
        store.commit(
            &competing,
            &b_candidate.root,
            b.len(),
            &Flatfs::open(&b_candidate.path).unwrap()
        ),
        Err(Error::Conflict)
    ));
    let first_path = block_path(&store.path, &a_candidate.root);
    let inode = fs::metadata(&first_path).unwrap().ino();
    let raw = fs::read(&first_path).unwrap();
    let repeated = store
        .commit(
            &a_intent,
            &a_candidate.root,
            a.len(),
            &Flatfs::open(&a_candidate.path).unwrap(),
        )
        .unwrap();
    assert_eq!(repeated, receipts[0]);
    assert_eq!(fs::metadata(&first_path).unwrap().ino(), inode);
    assert_eq!(fs::read(first_path).unwrap(), raw);
    assert!(store.orphans().unwrap().unreferenced_blocks.is_empty());
    check_database(&store);
}

#[test]
#[ignore = "requires the locked INT11 patched writer probe"]
fn actual_storage_failure_preserves_existing_receipt_and_reuse_refuses_corruption() {
    let temp = tempfile::tempdir().unwrap();
    let a = content(CHUNK + 17, 33);
    let b = content(CHUNK + 17, 44);
    let a_candidate = make_candidate(&temp, "a", &a, "INT12_PROBE");
    let b_candidate = make_candidate(&temp, "b", &b, "INT12_PROBE");
    let tail = make_candidate(&temp, "tail", &b[CHUNK..], "INT12_PROBE");
    let store = separate_store(&temp, "target");
    let a_intent = intent("first", &a);
    let b_intent = intent("later", &b);
    let a_stage = Flatfs::open(&a_candidate.path).unwrap();
    let b_stage = Flatfs::open(&b_candidate.path).unwrap();
    let first = store
        .commit(&a_intent, &a_candidate.root, a.len(), &a_stage)
        .unwrap();
    let a_root = block_path(&store.path, &first.root);
    let initial_inode = fs::metadata(&a_root).unwrap().ino();
    let initial_bytes = fs::read(&a_root).unwrap();

    let obstruction = block_path(&store.path, &tail.root);
    fs::create_dir_all(&obstruction).unwrap();
    assert!(
        matches!(store.commit(&b_intent, &b_candidate.root, b.len(), &b_stage),
        Err(Error::Verification(VerifyError::Io(error))) if error.kind() == io::ErrorKind::InvalidData)
    );
    assert!(store.lookup(&b_intent).unwrap().is_none());
    assert_eq!(store.lookup(&a_intent).unwrap(), Some(first));
    assert_eq!(fs::metadata(&a_root).unwrap().ino(), initial_inode);
    assert_eq!(fs::read(&a_root).unwrap(), initial_bytes);
    assert_eq!(
        store.orphans().unwrap().unreferenced_blocks,
        [block_path(&store.path, &b_candidate.root)]
    );
    fs::remove_dir(&obstruction).unwrap();
    let second = store
        .commit(&b_intent, &b_candidate.root, b.len(), &b_stage)
        .unwrap();
    assert_eq!(store.lookup(&b_intent).unwrap(), Some(second));

    let mut malformed = initial_bytes;
    malformed[0] ^= 1;
    fs::write(&a_root, &malformed).unwrap();
    assert!(matches!(
        store.lookup(&a_intent),
        Err(Error::Verification(VerifyError::HashMismatch(_)))
    ));
    assert!(matches!(
        store.commit(&a_intent, &a_candidate.root, a.len(), &a_stage),
        Err(Error::Verification(VerifyError::HashMismatch(_)))
    ));
    assert!(matches!(
        store.commit(
            &intent("other-scope", &a),
            &a_candidate.root,
            a.len(),
            &a_stage
        ),
        Err(Error::Verification(VerifyError::HashMismatch(_)))
    ));
    assert_eq!(fs::read(&a_root).unwrap(), malformed);
    assert_eq!(fs::metadata(a_root).unwrap().ino(), initial_inode);
    check_database(&store);
}

#[test]
#[ignore = "requires both locked INT11 writer probes"]
fn authoritative_digest_and_shared_int07_int10_limit_gate_completion() {
    assert_eq!(Limits::default().output_bytes, MAX_PUBLICATION_BYTES);
    assert_eq!(MAX_PUBLICATION_BYTES, 1024 * 1024);
    let temp = tempfile::tempdir().unwrap();

    let at_cap = content(MAX_PUBLICATION_BYTES, 16);
    let passed = make_candidate(&temp, "at-cap", &at_cap, "INT12_PROBE");
    let good = separate_store(&temp, "at-cap-target");
    let approved = intent("at-cap", &at_cap);
    let receipt = good
        .commit(
            &approved,
            &passed.root,
            at_cap.len(),
            &Flatfs::open(&passed.path).unwrap(),
        )
        .unwrap();
    assert_eq!(receipt.bytes, MAX_PUBLICATION_BYTES);

    let over_cap = content(MAX_PUBLICATION_BYTES + 1, 17);
    let excess = make_candidate(&temp, "over-cap", &over_cap, "INT12_PROBE");
    let target = separate_store(&temp, "over-cap-target");
    let stage = Flatfs::open(&excess.path).unwrap();
    let rejected = intent("oversize", &over_cap);
    assert!(matches!(
        target.commit(&rejected, &excess.root, over_cap.len(), &stage),
        Err(Error::SizeLimit)
    ));
    assert!(target.orphans().unwrap().unreferenced_blocks.is_empty());
    assert!(matches!(
        target.commit(&rejected, &excess.root, MAX_PUBLICATION_BYTES, &stage),
        Err(Error::Verification(VerifyError::Limit("output bytes")))
    ));
    assert!(target.lookup(&rejected).unwrap().is_none());

    let bytes = content(3 * CHUNK + 1, 18);
    let prefix = make_candidate(&temp, "pinned-truncation", &bytes, "INT12_ORIGINAL_PROBE");
    let truncated = separate_store(&temp, "truncated-target");
    let whole = intent("original-writer", &bytes);
    assert!(matches!(
        truncated.commit(
            &whole,
            &prefix.root,
            bytes.len(),
            &Flatfs::open(&prefix.path).unwrap()
        ),
        Err(Error::Verification(VerifyError::DigestMismatch))
    ));
    assert!(truncated.lookup(&whole).unwrap().is_none());
    assert!(!truncated.orphans().unwrap().unreferenced_blocks.is_empty());
}

#[test]
fn managed_store_identity_and_unrecognized_paths_are_checked() {
    let temp = tempfile::tempdir().unwrap();
    let store = separate_store(&temp, "store");
    assert!(matches!(
        Store::open(&store.path, "other"),
        Err(Error::InvalidStore)
    ));
    let shard = store.path.join("blocks").join("AA");
    fs::create_dir(&shard).unwrap();
    let unknown = format!("A{}.data", "é".repeat(27));
    fs::write(shard.join(unknown), b"unmanaged").unwrap();
    let inventory = store.orphans().unwrap();
    assert!(inventory.unreferenced_blocks.is_empty());
    assert!(inventory.abandoned_temporary_files.is_empty());
    fs::remove_file(store.path.join("completion.sqlite")).unwrap();
    assert!(matches!(
        store.lookup(&intent("unknown", b"")),
        Err(Error::Sql(_))
    ));
    assert!(!store.path.join("completion.sqlite").exists());
}
