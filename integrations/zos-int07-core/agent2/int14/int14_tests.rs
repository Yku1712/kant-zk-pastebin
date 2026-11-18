// SPDX-License-Identifier: Apache-2.0
use super::*;
use zos_int08_authorized_artifact_bridge::{AuthoritativeSource, AuthoritySnapshot};

#[derive(Clone, Copy, Debug)]
enum Change {
    Revocation,
    Expiry,
    Unavailable,
    ConcurrentRegrant,
}

const CHANGES: [Change; 4] = [
    Change::Revocation,
    Change::Expiry,
    Change::Unavailable,
    Change::ConcurrentRegrant,
];

struct Authority {
    lease: TestReleaseAuthority,
    unavailable: AtomicBool,
    calls: AtomicUsize,
    change_at_sample: Option<(usize, Change)>,
}

impl Authority {
    fn new(intent: &Intent) -> Self {
        Self {
            lease: TestReleaseAuthority::new(intent),
            unavailable: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            change_at_sample: None,
        }
    }

    fn change(&self, change: Change) {
        // The publishing thread waits until the independent authority thread finishes.
        thread::scope(|scope| {
            scope
                .spawn(|| match change {
                    Change::Revocation => self.lease.revoked.store(true, SeqCst),
                    Change::Expiry => self.lease.now.store(self.lease.expires, SeqCst),
                    Change::Unavailable => self.unavailable.store(true, SeqCst),
                    Change::ConcurrentRegrant => {
                        self.lease.revoked.store(true, SeqCst);
                        assert!(!self.lease.permits(&self.lease.approved).unwrap());
                        self.lease.revoked.store(false, SeqCst);
                    }
                })
                .join()
                .unwrap();
        });
    }
}

impl PublicationEligibility for Authority {
    fn permits(&self, intent: &Intent) -> Result<bool, Error> {
        let call = self.calls.fetch_add(1, SeqCst) + 1;
        let result = if self.unavailable.load(SeqCst) {
            Err(Error::ExternalUnavailable)
        } else {
            self.lease.permits(intent)
        };
        if let Some((selected, change)) = self.change_at_sample
            && selected == call
        {
            assert!(matches!(result, Ok(true)));
            self.change(change);
        }
        result
    }
}

fn rejected(result: Result<OperationState, Error>, change: Change) {
    match change {
        Change::Revocation | Change::Expiry => assert!(matches!(result, Err(Error::Ineligible))),
        Change::Unavailable => assert!(matches!(result, Err(Error::ExternalUnavailable))),
        Change::ConcurrentRegrant => assert!(matches!(result, Ok(OperationState::Confirmed(_)))),
    }
}

struct BeforeRead<'a> {
    source: ContractCompatibleProvider,
    callback: &'a dyn Fn(),
}

impl AuthoritativeSource for BeforeRead<'_> {
    fn snapshot(
        &self,
        principal: &AuthenticatedPrincipal,
        requested: &ArtifactRevision,
    ) -> Result<AuthoritySnapshot, Error> {
        (self.callback)();
        self.source.snapshot(principal, requested)
    }
}

#[test]
#[ignore = "requires corrected INT11 probe"]
fn authority_matrix() {
    for boundary in [
        "before-check",
        "before-source",
        "staged",
        "before-commit",
        "after-commit",
        "during-lookup",
        "historical",
    ] {
        for change in CHANGES {
            let f = Fixture::new();
            let bytes = body(44);
            let intent = intent(&bytes);
            let authority = Authority::new(&intent);
            let user = principal("tenant-a");
            let normal = observed(&f.path, &|_, _, _| Ok(()));
            let mut journal = bridge(&f.path);
            let priming_source = ArtifactMcpBridge::new(fixture_provider(&intent, bytes.clone()));
            let historical = if boundary == "historical" {
                Some(
                    normal
                        .execute(
                            &mut journal,
                            &priming_source,
                            &user,
                            &authority,
                            &intent,
                            Recovery::LookupOnly,
                        )
                        .unwrap(),
                )
            } else {
                None
            };
            if boundary == "during-lookup" {
                let lost = observed(&f.path, &|phase, _, _| {
                    if phase == Phase::ProviderCommitted {
                        Err(Error::ExternalUnavailable)
                    } else {
                        Ok(())
                    }
                });
                assert!(matches!(
                    lost.execute(
                        &mut journal,
                        &priming_source,
                        &user,
                        &authority,
                        &intent,
                        Recovery::LookupOnly
                    ),
                    Err(Error::ExternalUnavailable)
                ));
                assert_eq!(
                    journal.state(&intent).unwrap(),
                    Some(OperationState::Unresolved)
                );
            }
            let hit = AtomicBool::new(false);
            let apply = || {
                if !hit.swap(true, SeqCst) {
                    authority.change(change);
                }
            };
            if matches!(boundary, "before-check" | "historical") {
                apply();
            }
            if boundary == "before-source" {
                // Model an earlier successful policy decision outside INT08's authoritative read.
                assert!(authority.permits(&intent).unwrap());
            }
            let before_read = || {
                if boundary == "before-source" {
                    apply();
                }
            };
            let source = ArtifactMcpBridge::new(BeforeRead {
                source: fixture_provider(&intent, bytes),
                callback: &before_read,
            });
            let observe = |phase, _: &Path, _: &Cid| {
                if matches!(
                    (boundary, phase),
                    ("staged", Phase::Staged)
                        | ("before-commit", Phase::BeforeChildRead)
                        | ("after-commit", Phase::ProviderCommitted)
                ) {
                    if !hit.load(SeqCst) {
                        assert_eq!(
                            receipt_rows(&f.path),
                            i64::from(phase == Phase::ProviderCommitted)
                        );
                    }
                    apply();
                }
                Ok(())
            };
            let destination = observed(&f.path, &observe);
            let result = if boundary == "during-lookup" {
                let reconciliation = ObserveLookup {
                    destination: &destination,
                    callback: &apply,
                };
                let evidence = UnixfsReceiptEvidence {
                    original: LocalEvidence(&destination),
                    blocks: Flatfs::open(f.path.join("repo")).unwrap(),
                    limits: Limits::default(),
                };
                Publisher {
                    source: &source,
                    eligibility: &authority,
                    destination: &reconciliation,
                    evidence: &evidence,
                }
                .execute(&mut journal, &user, &intent, Recovery::LookupOnly)
            } else {
                destination.execute(
                    &mut journal,
                    &source,
                    &user,
                    &authority,
                    &intent,
                    Recovery::LookupOnly,
                )
            };
            assert!(hit.load(SeqCst));
            let state = journal.state(&intent).unwrap();
            let receipts = receipt_rows(&f.path);
            let regrant = matches!(change, Change::ConcurrentRegrant);
            rejected(result, change);
            if regrant {
                assert!(matches!(state, Some(OperationState::Confirmed(_))));
                assert_eq!(receipts, 1);
            } else if let Some(original) = historical {
                assert_eq!(state, Some(original));
                assert_eq!(receipts, 1);
            } else if matches!(boundary, "before-check" | "before-source") {
                assert_eq!(state, None);
                assert_eq!(receipts, 0);
            } else {
                assert_eq!(state, Some(OperationState::Unresolved));
                assert_eq!(receipts, 1);
            }
            println!("{boundary} {change:?}: journal={state:?}, receipts={receipts}");
        }
    }
}

struct ObserveLookup<'a, 'b> {
    destination: &'a LocalDestination<'b>,
    callback: &'a dyn Fn(),
}

impl Destination for ObserveLookup<'_, '_> {
    fn id(&self) -> &str {
        self.destination.id()
    }

    fn submit(&self, _: &Intent, _: &[u8]) -> Result<ReceiptCandidate, Error> {
        panic!("reconciliation unexpectedly attempted publication");
    }

    fn lookup(&self, intent: &Intent) -> Result<Option<ReceiptCandidate>, Error> {
        let result = self.destination.lookup(intent);
        (self.callback)();
        result
    }
}

#[test]
#[ignore = "requires corrected INT11 probe"]
fn authorization_sampling_races_with_confirmation_and_return() {
    // Calls: initial snapshot check, pre-dispatch, pre-journal-confirm, before wrapper return.
    for call in [3, 4] {
        for change in [Change::Revocation, Change::Expiry, Change::Unavailable] {
            let f = Fixture::new();
            let bytes = body(48);
            let intent = intent(&bytes);
            let source = ArtifactMcpBridge::new(fixture_provider(&intent, bytes));
            let mut authority = Authority::new(&intent);
            authority.change_at_sample = Some((call, change));
            let normal = observed(&f.path, &|_, _, _| Ok(()));
            let mut journal = bridge(&f.path);
            let result = normal.execute(
                &mut journal,
                &source,
                &principal("tenant-a"),
                &authority,
                &intent,
                Recovery::LookupOnly,
            );
            if call == 3 {
                rejected(result, change);
            } else {
                assert!(matches!(result, Ok(OperationState::Confirmed(_))));
            }
            assert!(matches!(
                journal.state(&intent).unwrap(),
                Some(OperationState::Confirmed(_))
            ));
            assert_eq!(receipt_rows(&f.path), 1);
            println!(
                "stale positive sample #{call} {change:?}: historical CONFIRMED, returned={}",
                call == 4
            );
        }
    }
}

fn same_error(actual: &Error, expected: &Error) -> bool {
    std::mem::discriminant(actual) == std::mem::discriminant(expected)
}

#[test]
#[ignore = "requires corrected INT11 probe"]
fn historical_receipt_survives_storage_fault_and_atomic_repair() {
    for fault in ["missing-root", "missing-child", "corrupt-child"] {
        let f = Fixture::new();
        let bytes = body(81);
        let intent = intent(&bytes);
        let source = ArtifactMcpBridge::new(fixture_provider(&intent, bytes));
        let user = principal("tenant-a");
        let authority = Authority::new(&intent);
        let calls = AtomicUsize::new(0);
        let count_staging = |phase, _: &Path, _: &Cid| {
            if phase == Phase::Staged {
                calls.fetch_add(1, SeqCst);
            }
            Ok(())
        };
        let normal = observed(&f.path, &count_staging);
        let mut journal = bridge(&f.path);
        let run = |journal: &mut Journal, recovery| {
            normal.execute(journal, &source, &user, &authority, &intent, recovery)
        };
        let state = run(&mut journal, Recovery::LookupOnly).unwrap();
        let OperationState::Confirmed(receipt) = &state else {
            panic!("setup")
        };
        assert_eq!(run(&mut journal, Recovery::LookupOnly).unwrap(), state);
        let repo = f.path.join("repo");
        let root = receipt.ipfs().unwrap().cid();
        let target = if fault == "missing-root" {
            flatfs_path(&repo, root)
        } else {
            child(&repo, root)
        };
        let exact = fs::read(&target).unwrap();
        let expected_error = if fault == "corrupt-child" {
            let mut corrupted = exact.clone();
            corrupted[0] ^= 1;
            fs::write(&target, corrupted).unwrap();
            Error::InvalidReceipt
        } else {
            fs::remove_file(&target).unwrap();
            Error::ExternalUnavailable
        };
        let damaged = fs::read(&target).ok();
        for recovery in [
            Recovery::LookupOnly,
            Recovery::RetryIdempotently,
            Recovery::LookupOnly,
        ] {
            assert!(same_error(
                &run(&mut journal, recovery).unwrap_err(),
                &expected_error
            ));
            assert_eq!(fs::read(&target).ok(), damaged);
            assert_eq!(journal.state(&intent).unwrap(), Some(state.clone()));
        }
        let mut temp = tempfile::NamedTempFile::new_in(target.parent().unwrap()).unwrap();
        use std::io::Write;
        temp.write_all(&exact).unwrap();
        temp.as_file().sync_all().unwrap();
        let start = std::sync::Barrier::new(2);
        thread::scope(|scope| {
            scope.spawn(|| {
                start.wait();
                for _ in 0..12 {
                    let result = run(&mut bridge(&f.path), Recovery::LookupOnly);
                    match result {
                        Ok(current) => assert_eq!(current, state),
                        Err(error) => assert!(same_error(&error, &expected_error)),
                    }
                }
            });
            start.wait();
            temp.persist(&target).unwrap();
            fs::File::open(target.parent().unwrap())
                .unwrap()
                .sync_all()
                .unwrap();
        });
        assert_eq!(run(&mut journal, Recovery::LookupOnly).unwrap(), state);
        assert_eq!(fs::read(target).unwrap(), exact);
        assert_eq!(journal.state(&intent).unwrap(), Some(state));
        assert_eq!(calls.load(SeqCst), 1);
        assert_eq!(receipt_rows(&f.path), 1);
        check_databases(&f.path);
        println!(
            "{fault}: no rewrite on lookup/retry; atomic repair restored readback; historical receipt unchanged"
        );
    }
}

#[test]
#[ignore = "requires corrected INT11 probe"]
fn shared_blocks_and_inventory_during_publication() {
    let f = Fixture::new();
    let bytes = body(75);
    let first = intent(&bytes);
    let tail = body(76);
    let second = Intent::new(
        ArtifactRevision {
            revision: first.artifact().revision + 1,
            digest: ContentDigest::of(&tail),
            ..first.artifact().clone()
        },
        STORE_ID.to_owned(),
    )
    .unwrap();
    let initial = observed(&f.path, &|_, _, _| Ok(()));
    let source = ArtifactMcpBridge::new(fixture_provider(&first, bytes));
    let one = initial
        .execute(
            &mut bridge(&f.path),
            &source,
            &principal("tenant-a"),
            &Authority::new(&first),
            &first,
            Recovery::LookupOnly,
        )
        .unwrap();
    let next = ArtifactMcpBridge::new(fixture_provider(&second, tail));
    let (ready_tx, ready_rx) = sync_channel(0);
    let (continue_tx, continue_rx) = sync_channel(0);
    let continue_rx = Mutex::new(continue_rx);
    let paused = AtomicBool::new(false);
    let observe = |phase, _: &Path, _: &Cid| {
        if phase == Phase::BeforeChildRead && !paused.swap(true, SeqCst) {
            ready_tx.send(()).unwrap();
            continue_rx.lock().unwrap().recv_timeout(WAIT).unwrap();
        }
        Ok(())
    };
    thread::scope(|scope| {
        let writer = scope.spawn(|| {
            observed(&f.path, &observe).execute(
                &mut bridge(&f.path),
                &next,
                &principal("tenant-a"),
                &Authority::new(&second),
                &second,
                Recovery::LookupOnly,
            )
        });
        ready_rx.recv_timeout(WAIT).unwrap();
        assert!(initial.lookup(&first).unwrap().is_some());
        assert!(initial.lookup(&second).unwrap().is_none());
        let (entered_tx, entered_rx) = sync_channel(0);
        let (done_tx, done_rx) = sync_channel(1);
        let initial = &initial;
        let inventory = scope.spawn(move || {
            entered_tx.send(()).unwrap();
            done_tx.send(initial.store.orphans()).unwrap();
        });
        entered_rx.recv_timeout(WAIT).unwrap();
        let pending = done_rx.recv_timeout(Duration::from_millis(100));
        continue_tx.send(()).unwrap();
        assert!(matches!(
            pending,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(matches!(
            writer.join().unwrap().unwrap(),
            OperationState::Confirmed(_)
        ));
        assert!(
            done_rx
                .recv_timeout(WAIT)
                .unwrap()
                .unwrap()
                .unreferenced_blocks
                .is_empty()
        );
        inventory.join().unwrap();
    });
    let db = Connection::open(f.path.join("repo/completion.sqlite")).unwrap();
    let shared: u32 = db.query_row("SELECT COUNT(*) FROM (SELECT key FROM receipt_blocks GROUP BY key HAVING COUNT(*) > 1)", [], |r| r.get(0)).unwrap();
    assert!(shared > 0);
    assert_eq!(bridge(&f.path).state(&first).unwrap(), Some(one));
    assert_eq!(receipt_rows(&f.path), 2);
    assert_eq!(fs::read_dir(f.path.join("stages")).unwrap().count(), 0);
    println!(
        "{shared} canonical block keys shared; concurrent reader saw old graph and no partial receipt; inventory serialized with writer"
    );
}

#[test]
#[ignore = "requires corrected INT11 probe"]
fn abrupt_exit_staging_quarantine_is_separate_from_committed_blocks() {
    for boundary in ["before", "after"] {
        let f = Fixture::new();
        let bytes = body(21);
        let intent = intent(&bytes);
        let normal = observed(&f.path, &|_, _, _| Ok(()));
        let _ = bridge(&f.path);
        let mut proc = Command::new(env::current_exe().unwrap())
            .args(["--ignored", "--exact", "tests::crash_child"])
            .env("INT13_CRASH_PATH", &f.path)
            .env("INT13_CRASH_BOUNDARY", boundary)
            .spawn()
            .unwrap();
        let limit = Instant::now() + WAIT;
        let marker = loop {
            if let Ok(root) = fs::read_to_string(f.path.join("ready")) {
                break Some(root);
            }
            if proc.try_wait().unwrap().is_some() || Instant::now() >= limit {
                break None;
            }
            thread::sleep(Duration::from_millis(10));
        };
        let _ = proc.kill();
        let status = proc.wait().unwrap();
        assert_eq!(status.signal(), Some(9));
        let root: Cid = marker.unwrap().parse().unwrap();
        let canonical = flatfs_path(&f.path.join("repo"), &root);
        let canonical_bytes = fs::read(&canonical).unwrap();
        check_databases(&f.path);
        let staged: Vec<_> = fs::read_dir(f.path.join("stages"))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(staged.len(), 1);
        assert_eq!(
            fs::read(staged[0].path().join("artifact")).unwrap(),
            body(21)
        );
        // This fixture owns the only worker and has joined it after SIGKILL at a
        // post-probe hook. Quarantine requires that stronger precondition.
        let quarantine = f.path.join("quarantined");
        fs::rename(staged[0].path(), &quarantine).unwrap();
        assert!(quarantine.join("artifact").is_file());
        assert_eq!(fs::read(canonical).unwrap(), canonical_bytes);
        let inventory = normal.store.orphans().unwrap();
        assert_eq!(
            inventory.unreferenced_blocks.len(),
            usize::from(boundary == "before")
        );
        assert_eq!(receipt_rows(&f.path), i64::from(boundary == "after"));
        assert_eq!(
            bridge(&f.path).state(&intent).unwrap(),
            Some(OperationState::Unresolved)
        );
        assert_eq!(
            normal.lookup(&intent).unwrap().is_some(),
            boundary == "after"
        );
        let source = ArtifactMcpBridge::new(fixture_provider(&intent, body(21)));
        for retry in [
            Recovery::RetryIdempotently,
            Recovery::RetryIdempotently,
            Recovery::LookupOnly,
        ] {
            assert!(matches!(
                normal.execute(
                    &mut bridge(&f.path),
                    &source,
                    &principal("tenant-a"),
                    &Authority::new(&intent),
                    &intent,
                    retry
                ),
                Ok(OperationState::Confirmed(_))
            ));
        }
        assert_eq!(receipt_rows(&f.path), 1);
        assert!(
            normal
                .store
                .orphans()
                .unwrap()
                .unreferenced_blocks
                .is_empty()
        );
        assert_eq!(fs::read_dir(f.path.join("stages")).unwrap().count(), 0);
        println!(
            "{boundary}: pid={} exit_code={:?} signal={:?}, staged plaintext quarantined, canonical root preserved, one receipt after retries",
            proc.id(),
            status.code(),
            status.signal()
        );
    }
}

struct MutableSource(Mutex<ContractCompatibleProvider>);

impl AuthoritativeSource for &MutableSource {
    fn snapshot(
        &self,
        principal: &AuthenticatedPrincipal,
        requested: &ArtifactRevision,
    ) -> Result<AuthoritySnapshot, Error> {
        self.0.lock().unwrap().snapshot(principal, requested)
    }
}

#[test]
#[ignore = "requires corrected INT11 probe"]
fn source_revocation_after_snapshot_only_affects_later_execution() {
    let f = Fixture::new();
    let bytes = body(90);
    let intent = intent(&bytes);
    let provider = MutableSource(Mutex::new(fixture_provider(&intent, bytes)));
    let source = ArtifactMcpBridge::new(&provider);
    let observe = |phase, _: &Path, _: &Cid| {
        if phase == Phase::Staged {
            thread::scope(|scope| {
                scope
                    .spawn(|| provider.0.lock().unwrap().set_revoked(true))
                    .join()
                    .unwrap();
            });
        }
        Ok(())
    };
    let destination = observed(&f.path, &observe);
    let lease = Authority::new(&intent);
    let user = principal("tenant-a");
    let mut journal = bridge(&f.path);
    let confirmed = destination
        .execute(
            &mut journal,
            &source,
            &user,
            &lease,
            &intent,
            Recovery::LookupOnly,
        )
        .unwrap();
    assert!(matches!(confirmed, OperationState::Confirmed(_)));
    assert!(matches!(
        destination.execute(
            &mut journal,
            &source,
            &user,
            &lease,
            &intent,
            Recovery::LookupOnly
        ),
        Err(Error::AccessDenied)
    ));
    assert_eq!(journal.state(&intent).unwrap(), Some(confirmed));
    assert_eq!(receipt_rows(&f.path), 1);
    println!(
        "INT08 source revocation after snapshot: in-flight return confirmed; next execution denied"
    );
}
