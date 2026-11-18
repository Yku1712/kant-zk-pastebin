// SPDX-License-Identifier: Apache-2.0
//! Defensive regressions. Database changes are fixtures in disposable SQLite files.

use super::{
    pinned_regressions::{cid, paused_provider, publish_fixture},
    *,
};
use std::{sync::RwLock, time::Instant};
use zos_int07_core::{DurableRetry, MAX_PUBLICATION_BYTES};

#[test]
fn corrupted_receipt_and_schema_fail_closed_and_schema_rejects_inconsistent_rows() {
    let intent = intent();
    let scratch = Scratch::new();
    publish_fixture(&scratch, &intent, None);
    let connection = Connection::open(scratch.journal_path()).unwrap();
    for remote in ["\n".to_string(), "é".repeat(257)] {
        assert_eq!(
            connection
                .execute("UPDATE publication SET remote_id = ?1", [&remote])
                .unwrap(),
            1
        );
        assert!(matches!(
            scratch.journal().state(&intent),
            Err(Error::JournalConflict)
        ));
    }
    for inconsistent in [
        "UPDATE publication SET remote_id = NULL",
        "UPDATE publication SET state = 'UNRECOGNIZED'",
        "UPDATE publication SET state = 'PREPARED'",
    ] {
        let error = connection.execute(inconsistent, []).unwrap_err();
        assert_eq!(
            error.sqlite_error_code(),
            Some(ErrorCode::ConstraintViolation)
        );
    }
    connection.pragma_update(None, "user_version", 99).unwrap();
    assert!(matches!(
        Journal::open(scratch.journal_path()),
        Err(Error::JournalConflict)
    ));
}

#[test]
fn publication_budget_rejects_oversize_before_journaling_and_accepts_exact_limit() {
    for size in [MAX_PUBLICATION_BYTES, MAX_PUBLICATION_BYTES + 1] {
        let body = vec![0x61; size];
        let revision = ArtifactRevision {
            digest: ContentDigest::of(&body),
            ..revision()
        };
        let intent = Intent::new(revision.clone(), DESTINATION.into()).unwrap();
        let source = Source {
            revision,
            release: Release::ExplicitlyPublic,
            body,
        };
        let provider = Provider {
            submission: Some(receipt(&intent)),
            ..Provider::default()
        };
        let evidence = Evidence {
            bytes: &source.body,
            ..Evidence::VALID
        };
        let scratch = Scratch::new();
        let mut journal = scratch.journal();
        let result = publisher(&source, &ReleaseRule(intent.clone()), &provider, &evidence)
            .execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly);
        if size == MAX_PUBLICATION_BYTES {
            assert!(matches!(result, Ok(OperationState::Confirmed(_))));
            assert_eq!(provider.submits.load(SeqCst), 1);
        } else {
            assert!(matches!(result, Err(Error::ResourceLimit)));
            assert_eq!(journal.state(&intent).unwrap(), None);
            assert_eq!(provider.submits.load(SeqCst), 0);
        }
    }
}

struct OversizedEvidence {
    ipfs: bool,
}

fn stream_beyond_limit(out: &mut dyn EvidenceOutput) -> Result<(), Error> {
    let block = [0_u8; 4096];
    for _ in 0..MAX_PUBLICATION_BYTES / block.len() {
        out.append(&block)?;
    }
    assert_eq!(out.remaining(), 0);
    assert!(matches!(out.append(b"x"), Err(Error::ResourceLimit)));
    // Intentionally drop the fixture's append error to exercise the sticky bound.
    Ok(())
}

impl ReceiptEvidence for OversizedEvidence {
    fn published_bytes(
        &self,
        _: &Intent,
        _: &ReceiptCandidate,
        out: &mut dyn EvidenceOutput,
    ) -> Result<(), Error> {
        if self.ipfs {
            out.append(PAYLOAD)
        } else {
            stream_beyond_limit(out)
        }
    }

    fn verified_ipfs_bytes(
        &self,
        _: &Intent,
        _: &Cid,
        out: &mut dyn EvidenceOutput,
    ) -> Result<(), Error> {
        assert!(self.ipfs);
        stream_beyond_limit(out)
    }
}

#[test]
fn oversized_readback_cannot_confirm_even_if_adapter_ignores_the_budget_error() {
    let intent = intent();
    for ipfs in [false, true] {
        let scratch = Scratch::new();
        let mut journal = scratch.journal();
        let mut candidate = receipt(&intent);
        candidate.ipfs_cid = ipfs.then(|| cid(0x55, 0x12, &intent.artifact().digest.0));
        let caller = Publisher {
            source: &Source::default(),
            eligibility: &ReleaseRule(intent.clone()),
            destination: &Provider {
                submission: Some(candidate),
                ..Provider::default()
            },
            evidence: &OversizedEvidence { ipfs },
        };
        assert!(matches!(
            caller.execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly),
            Err(Error::ResourceLimit)
        ));
        assert_eq!(
            journal.state(&intent).unwrap(),
            Some(OperationState::Unresolved)
        );
    }
}

struct TestBlock {
    body: &'static [u8],
    links: &'static [usize],
}

struct WalkBudget {
    nodes: usize,
    depth: usize,
    bytes: usize,
    deadline: Instant,
}

fn visit_fixture(
    blocks: &[TestBlock],
    index: usize,
    depth: usize,
    budget: &mut WalkBudget,
    out: &mut dyn EvidenceOutput,
) -> Result<(), Error> {
    if budget.nodes == 0 || depth > budget.depth || Instant::now() >= budget.deadline {
        return Err(Error::ResourceLimit);
    }
    budget.nodes -= 1;
    let block = blocks.get(index).ok_or(Error::ExternalUnavailable)?;
    // Charge the fixture's one-byte link entries as well as its body bytes.
    budget.bytes = budget
        .bytes
        .checked_sub(block.body.len() + block.links.len())
        .ok_or(Error::ResourceLimit)?;
    out.append(block.body)?;
    for link in block.links {
        visit_fixture(blocks, *link, depth + 1, budget, out)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct SimulatedTraversal {
    node_cap: usize,
    depth_cap: usize,
    byte_cap: usize,
    expired: bool,
    missing_leaf: bool,
}

impl ReceiptEvidence for SimulatedTraversal {
    fn published_bytes(
        &self,
        _: &Intent,
        _: &ReceiptCandidate,
        out: &mut dyn EvidenceOutput,
    ) -> Result<(), Error> {
        out.append(PAYLOAD)
    }

    fn verified_ipfs_bytes(
        &self,
        _: &Intent,
        _: &Cid,
        out: &mut dyn EvidenceOutput,
    ) -> Result<(), Error> {
        let blocks = [
            TestBlock {
                body: &PAYLOAD[..5],
                links: &[1],
            },
            TestBlock {
                body: &PAYLOAD[5..],
                links: &[],
            },
        ];
        let blocks = if self.missing_leaf {
            &blocks[..1]
        } else {
            &blocks
        };
        let now = Instant::now();
        let mut budget = WalkBudget {
            nodes: self.node_cap,
            depth: self.depth_cap,
            bytes: self.byte_cap,
            deadline: if self.expired {
                now
            } else {
                now + TEST_TIMEOUT
            },
        };
        visit_fixture(blocks, 0, 0, &mut budget, out)
    }
}

#[test]
fn simulated_traversal_requires_complete_payload_and_bounds_work() {
    let evidence = SimulatedTraversal {
        node_cap: 2,
        depth_cap: 1,
        byte_cap: PAYLOAD.len() + 1,
        expired: false,
        missing_leaf: false,
    };
    let intent = intent();
    let mut candidate = receipt(&intent);
    candidate.ipfs_cid = Some(cid(0x70, 0x12, &ContentDigest::of(b"simulated DAG root").0));
    let provider = Provider {
        submission: Some(candidate),
        ..Provider::default()
    };
    let execute = |journal: &mut Journal, evidence: &SimulatedTraversal| {
        Publisher {
            source: &Source::default(),
            eligibility: &ReleaseRule(intent.clone()),
            destination: &provider,
            evidence,
        }
        .execute(journal, &Grant::CURRENT, &intent, Recovery::LookupOnly)
    };
    assert!(matches!(
        execute(&mut Scratch::new().journal(), &evidence),
        Ok(OperationState::Confirmed(_))
    ));

    for over_budget in [
        SimulatedTraversal {
            node_cap: 1,
            ..evidence
        },
        SimulatedTraversal {
            depth_cap: 0,
            ..evidence
        },
        SimulatedTraversal {
            byte_cap: PAYLOAD.len(),
            ..evidence
        },
        SimulatedTraversal {
            expired: true,
            ..evidence
        },
        SimulatedTraversal {
            missing_leaf: true,
            ..evidence
        },
    ] {
        let scratch = Scratch::new();
        let mut journal = scratch.journal();
        let result = execute(&mut journal, &over_budget);
        if over_budget.missing_leaf {
            assert!(matches!(result, Err(Error::ExternalUnavailable)));
        } else {
            assert!(matches!(result, Err(Error::ResourceLimit)));
        }
        assert_eq!(
            journal.state(&intent).unwrap(),
            Some(OperationState::Unresolved)
        );
    }
}

struct LeasedSource {
    approved: RwLock<bool>,
    source: Source,
}

impl AuthorizedSource for LeasedSource {
    type Principal = Grant<'static>;
    fn with_revision<T>(
        &self,
        grant: &Self::Principal,
        requested: &ArtifactRevision,
        run: impl FnOnce(AuthorizedSnapshot<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let release = self.approved.read().map_err(|_| Error::SourceUnavailable)?;
        if !*release {
            return Err(Error::Ineligible);
        }
        self.source.with_revision(grant, requested, run)
    }
}

#[test]
fn release_lease_serializes_revocation_with_dispatch_and_confirmation() {
    let intent = intent();
    let scratch = Scratch::new();
    let source = LeasedSource {
        approved: RwLock::new(true),
        source: Source::default(),
    };
    let (provider, started, release) = paused_provider(&intent);
    let caller = Publisher {
        source: &source,
        eligibility: &ReleaseRule(intent.clone()),
        destination: &provider,
        evidence: &Evidence::VALID,
    };
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            caller.execute(
                &mut scratch.journal(),
                &Grant::CURRENT,
                &intent,
                Recovery::LookupOnly,
            )
        });
        started.recv_timeout(TEST_TIMEOUT).unwrap();
        assert!(matches!(
            source.approved.try_write(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        release.send(()).unwrap();
        assert!(matches!(
            first.join().unwrap(),
            Ok(OperationState::Confirmed(_))
        ));
    });
    *source.approved.write().unwrap() = false;
    assert!(matches!(
        caller.execute(
            &mut scratch.journal(),
            &Grant::CURRENT,
            &intent,
            Recovery::LookupOnly
        ),
        Err(Error::Ineligible)
    ));
    assert_eq!(provider.submits.load(SeqCst), 1);
}

struct OtherCapability {
    id: &'static str,
    retries: AtomicUsize,
}

impl Destination for OtherCapability {
    fn id(&self) -> &str {
        self.id
    }
    fn submit(&self, _: &Intent, _: &[u8]) -> Result<ReceiptCandidate, Error> {
        Err(Error::ExternalUnavailable)
    }
    fn lookup(&self, _: &Intent) -> Result<Option<ReceiptCandidate>, Error> {
        Ok(None)
    }
}
impl DurableRetry for OtherCapability {
    fn retry_idempotently(&self, _: &Intent, _: &[u8]) -> Result<Option<ReceiptCandidate>, Error> {
        self.retries.fetch_add(1, SeqCst);
        Ok(None)
    }
}

struct Misbound<'a> {
    capability: &'a OtherCapability,
    provider: Provider,
}

impl Destination for Misbound<'_> {
    fn id(&self) -> &str {
        self.provider.id()
    }
    fn submit(&self, intent: &Intent, bytes: &[u8]) -> Result<ReceiptCandidate, Error> {
        self.provider.submit(intent, bytes)
    }
    fn lookup(&self, intent: &Intent) -> Result<Option<ReceiptCandidate>, Error> {
        self.provider.lookup(intent)
    }
    fn durable_retry(&self) -> Option<&dyn DurableRetry> {
        Some(self.capability)
    }
}

#[test]
fn unrelated_capability_does_not_grant_retry_even_with_same_label() {
    let scratch = Scratch::new();
    let intent = intent();
    let capability = OtherCapability {
        id: DESTINATION,
        retries: AtomicUsize::new(0),
    };
    let provider = Misbound {
        capability: &capability,
        provider: Provider::default(),
    };
    let caller = Publisher {
        source: &Source::default(),
        eligibility: &ReleaseRule(intent.clone()),
        destination: &provider,
        evidence: &Evidence::VALID,
    };
    let mut journal = scratch.journal();
    assert!(matches!(
        caller.execute(&mut journal, &Grant::CURRENT, &intent, Recovery::LookupOnly),
        Err(Error::ExternalUnavailable)
    ));
    assert!(matches!(
        caller.execute(
            &mut journal,
            &Grant::CURRENT,
            &intent,
            Recovery::RetryIdempotently
        ),
        Err(Error::InvalidBinding)
    ));
    assert_eq!(capability.retries.load(SeqCst), 0);
    assert_eq!(provider.provider.submits.load(SeqCst), 1);
    assert_eq!(
        journal.state(&intent).unwrap(),
        Some(OperationState::Unresolved)
    );
}
