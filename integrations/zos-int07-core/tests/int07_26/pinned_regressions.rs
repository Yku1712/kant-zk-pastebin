// SPDX-License-Identifier: Apache-2.0
//! Portable defensive regressions for both the pinned and hardened crate.

use super::*;

pub(super) fn publish_fixture(
    scratch: &Scratch,
    intent: &Intent,
    ipfs_cid: Option<String>,
) -> OperationState {
    let mut candidate = receipt(intent);
    candidate.ipfs_cid = ipfs_cid;
    let provider = Provider {
        submission: Some(candidate),
        ..Provider::default()
    };
    let evidence = Evidence {
        ipfs: Some(PAYLOAD),
        ..Evidence::VALID
    };
    publisher(
        &Source::default(),
        &ReleaseRule(intent.clone()),
        &provider,
        &evidence,
    )
    .execute(
        &mut scratch.journal(),
        &Grant::CURRENT,
        intent,
        Recovery::LookupOnly,
    )
    .unwrap()
}

pub(super) fn cid(codec: u64, hash: u64, digest: &[u8]) -> String {
    Cid::new_v1(codec, Multihash::wrap(hash, digest).unwrap()).to_string()
}

#[test]
fn restored_cid_uses_same_canonical_shape_and_raw_digest_as_live_evidence() {
    let intent = intent();
    let digest = intent.artifact().digest.0;
    let valid = cid(0x55, 0x12, &digest);
    let cases = [
        cid(0x71, 0x12, &digest),
        "malformed".into(),
        "x".repeat(257),
        format!("/ipfs/{valid}"),
        cid(0x70, 0x13, &digest),
        cid(0x70, 0x12, &digest[..31]),
        cid(0x55, 0x12, &ContentDigest::of(b"different").0),
    ];
    let scratch = Scratch::new();
    let confirmed = publish_fixture(&scratch, &intent, Some(valid.clone()));
    assert_eq!(scratch.journal().state(&intent).unwrap(), Some(confirmed));
    let raw = Connection::open(scratch.journal_path()).unwrap();

    for invalid in cases {
        assert_eq!(
            raw.execute("UPDATE publication SET ipfs_cid = ?1", [&invalid])
                .unwrap(),
            1
        );
        let mut restored = scratch.journal();
        assert!(
            matches!(restored.state(&intent), Err(Error::JournalConflict)),
            "{invalid}"
        );
        let provider = Provider::default();
        assert!(matches!(
            publisher(
                &Source::default(),
                &ReleaseRule(intent.clone()),
                &provider,
                &Evidence::VALID
            )
            .execute(
                &mut restored,
                &Grant::CURRENT,
                &intent,
                Recovery::RetryIdempotently
            ),
            Err(Error::JournalConflict)
        ));
        assert_eq!(provider.submits.load(SeqCst), 0);
        assert_eq!(provider.lookups.load(SeqCst), 0);
    }
}

pub(super) fn paused_provider(intent: &Intent) -> (Provider, Receiver<()>, SyncSender<()>) {
    let (entered, started) = sync_channel(1);
    let (release, resume) = sync_channel(1);
    let provider = Provider {
        submission: Some(receipt(intent)),
        gate: Some(DispatchGate {
            entered,
            resume: Mutex::new(resume),
        }),
        ..Provider::default()
    };
    (provider, started, release)
}

#[test]
fn policy_revoked_in_flight_prevents_confirmation_without_automatic_retry() {
    let scratch = Scratch::new();
    let intent = intent();
    let source = Source::default();
    let policy = RevocablePolicy {
        intent: intent.clone(),
        approved: AtomicBool::new(true),
    };
    let (provider, started, release) = paused_provider(&intent);
    let caller = Publisher {
        source: &source,
        eligibility: &policy,
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
        policy.approved.store(false, SeqCst);
        release.send(()).unwrap();
        assert!(matches!(first.join().unwrap(), Err(Error::Ineligible)));
    });
    let mut reopened = scratch.journal();
    assert_eq!(
        reopened.state(&intent).unwrap(),
        Some(OperationState::Unresolved)
    );
    assert!(matches!(
        caller.execute(
            &mut reopened,
            &Grant::CURRENT,
            &intent,
            Recovery::RetryIdempotently
        ),
        Err(Error::Ineligible)
    ));
    assert_eq!(provider.submits.load(SeqCst), 1);
    assert_eq!(provider.lookups.load(SeqCst), 0);
}
