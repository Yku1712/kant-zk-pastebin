// SPDX-License-Identifier: Apache-2.0
//! Contract-compatible synthetic provider tests.
//!
//! No test in this file runs Artifact MCP, Pastebin, IPFS, or any network service. The real
//! source audit is recorded separately in `AUTHORITY_CONTRACT.md`; these tests exercise the
//! typed boundary inferred from it.

use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
};
use zos_int07_core::{
    ArtifactRevision, AuthorizedSource, ContentDigest, Error, Intent, Journal, OperationState,
    PublicationEligibility, Publisher, ReceiptCandidate, ReceiptEvidence, Recovery,
};
use zos_int08_authorized_artifact_bridge::{
    ArtifactMcpBridge, ArtifactRecord, Classification, ContractCompatibleProvider,
    CredentialRecord, CredentialVerifier, Role,
};

const BODY: &[u8] = b"<p>int08 authorized public fixture</p>";
const DESTINATION: &str = "int08-test-destination";

fn binding() -> ArtifactRevision {
    ArtifactRevision {
        source: "artifact-mcp-source".into(),
        tenant: "org-a".into(),
        artifact: "artifact-a".into(),
        revision: 7,
        digest: ContentDigest::of(BODY),
    }
}

fn intent() -> Intent {
    Intent::new(binding(), DESTINATION.into()).unwrap()
}

fn verifier() -> CredentialVerifier {
    CredentialVerifier::new([
        (
            "good-key",
            CredentialRecord::api_key("org-a", "client-a", Role::Author),
        ),
        (
            "foreign-key",
            CredentialRecord::api_key("org-b", "client-b", Role::Author),
        ),
        (
            "reader-key",
            CredentialRecord::oauth("org-a", "reader-a", Role::Reader),
        ),
    ])
}

fn principal(credential: &str) -> zos_int08_authorized_artifact_bridge::AuthenticatedPrincipal {
    verifier().authenticate(credential).unwrap()
}

fn provider() -> ContractCompatibleProvider {
    ContractCompatibleProvider::empty().with_record(ArtifactRecord::new(
        binding(),
        Classification::Public,
        zos_int07_core::BodyKind::SingleFile,
        BODY.to_vec(),
    ))
}

fn snapshot_bytes(
    provider: &ContractCompatibleProvider,
    principal: &zos_int08_authorized_artifact_bridge::AuthenticatedPrincipal,
    requested: &ArtifactRevision,
) -> Result<Vec<u8>, Error> {
    ArtifactMcpBridge::new(provider.clone())
        .with_revision(principal, requested, |snapshot| Ok(snapshot.bytes.to_vec()))
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let suffix = NEXT.fetch_add(1, SeqCst);
            let path =
                std::env::temp_dir().join(format!("zos-int08-{}-{suffix}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test directory: {error}"),
            }
        }
    }

    fn journal(&self) -> Journal {
        Journal::open(self.0.join("journal.sqlite")).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct TestDestination {
    submits: AtomicUsize,
    receipt: Mutex<Option<ReceiptCandidate>>,
}

impl zos_int07_core::Destination for TestDestination {
    fn id(&self) -> &str {
        DESTINATION
    }

    fn submit(&self, intent: &Intent, body: &[u8]) -> Result<ReceiptCandidate, Error> {
        assert_eq!(body, BODY);
        self.submits.fetch_add(1, SeqCst);
        let receipt = ReceiptCandidate {
            operation: intent.operation(),
            remote_id: "int08-fixture".into(),
            local_alias: Some(intent.artifact().digest.pastebin_alias()),
            witness: intent.artifact().digest,
            ipfs_cid: None,
        };
        *self.receipt.lock().unwrap() = Some(receipt.clone());
        Ok(receipt)
    }

    fn lookup(&self, _: &Intent) -> Result<Option<ReceiptCandidate>, Error> {
        Ok(self.receipt.lock().unwrap().clone())
    }
}

struct Allow;
impl PublicationEligibility for Allow {
    fn permits(&self, _: &Intent) -> Result<bool, Error> {
        Ok(true)
    }
}

struct Evidence;
impl ReceiptEvidence for Evidence {
    fn published_bytes(&self, _: &Intent, _: &ReceiptCandidate) -> Result<Vec<u8>, Error> {
        Ok(BODY.to_vec())
    }

    fn verified_ipfs_bytes(&self, _: &Intent, _: &cid::Cid) -> Result<Vec<u8>, Error> {
        Err(Error::InvalidReceipt)
    }
}

#[test]
fn authorized_public_revision() {
    let snapshot = snapshot_bytes(&provider(), &principal("good-key"), &binding()).unwrap();
    assert_eq!(snapshot, BODY);
}

#[test]
fn unauthorized_principal() {
    assert!(verifier().authenticate("unknown-key").is_err());
    assert!(matches!(
        snapshot_bytes(&provider(), &principal("foreign-key"), &binding()),
        Err(Error::AccessDenied)
    ));
}

#[test]
fn cross_tenant_request() {
    let requested = ArtifactRevision {
        tenant: "org-b".into(),
        ..binding()
    };
    let source = ContractCompatibleProvider::empty().with_record(ArtifactRecord::new(
        requested.clone(),
        Classification::Public,
        zos_int07_core::BodyKind::SingleFile,
        BODY.to_vec(),
    ));
    assert!(matches!(
        snapshot_bytes(&source, &principal("good-key"), &requested),
        Err(Error::AccessDenied)
    ));
}

#[test]
fn stale_grant() {
    let mut source = provider();
    source.set_stale_grant(true);
    assert!(matches!(
        snapshot_bytes(&source, &principal("good-key"), &binding()),
        Err(Error::AccessDenied)
    ));
}

#[test]
fn revision_substitution() {
    let mut source = provider();
    source.set_substitute_revision(Some(8));
    assert!(matches!(
        snapshot_bytes(&source, &principal("good-key"), &binding()),
        Err(Error::InvalidBinding)
    ));
}

#[test]
fn digest_mismatch() {
    let mismatched = ArtifactRevision {
        digest: ContentDigest::of(b"other bytes"),
        ..binding()
    };
    let source = ContractCompatibleProvider::empty().with_record(ArtifactRecord::new(
        mismatched.clone(),
        Classification::Public,
        zos_int07_core::BodyKind::SingleFile,
        BODY.to_vec(),
    ));
    assert!(matches!(
        snapshot_bytes(&source, &principal("good-key"), &mismatched),
        Err(Error::DigestMismatch)
    ));
}

#[test]
fn public_to_private_policy_transition() {
    let mut source = provider();
    source.set_classification(&binding(), Classification::Private);
    assert!(matches!(
        snapshot_bytes(&source, &principal("good-key"), &binding()),
        Err(Error::Ineligible)
    ));
}

#[test]
fn missing_artifact() {
    let missing = ArtifactRevision {
        revision: 99,
        ..binding()
    };
    assert!(matches!(
        snapshot_bytes(&provider(), &principal("good-key"), &missing),
        Err(Error::SourceUnavailable)
    ));
}

#[test]
fn inconsistent_source_snapshot() {
    let mut source = provider();
    source.set_corrupt_snapshot(true);
    assert!(matches!(
        snapshot_bytes(&source, &principal("good-key"), &binding()),
        Err(Error::InvalidBinding)
    ));
}

#[test]
fn confirmed_operation_cannot_be_reused_after_revocation() {
    let scratch = Scratch::new();
    let source = provider();
    let bridge = ArtifactMcpBridge::new(source.clone());
    let destination = TestDestination::default();
    let publisher = Publisher {
        source: &bridge,
        eligibility: &Allow,
        destination: &destination,
        evidence: &Evidence,
    };
    let intent = intent();
    let state = publisher
        .execute(
            &mut scratch.journal(),
            &principal("good-key"),
            &intent,
            Recovery::LookupOnly,
        )
        .unwrap();
    let OperationState::Confirmed(receipt) = &state else {
        panic!("expected confirmed receipt")
    };
    assert_eq!(receipt.remote_id(), "int08-fixture");
    assert_eq!(destination.submits.load(SeqCst), 1);

    let mut revoked = source;
    revoked.set_revoked(true);
    let revoked_bridge = ArtifactMcpBridge::new(revoked);
    let publisher = Publisher {
        source: &revoked_bridge,
        eligibility: &Allow,
        destination: &destination,
        evidence: &Evidence,
    };
    assert!(matches!(
        publisher.execute(
            &mut scratch.journal(),
            &principal("good-key"),
            &intent,
            Recovery::LookupOnly
        ),
        Err(Error::AccessDenied)
    ));
    assert_eq!(destination.submits.load(SeqCst), 1);
}

#[test]
fn reader_with_read_access_still_lacks_public_release() {
    let snapshot = snapshot_bytes(&provider(), &principal("reader-key"), &binding());
    assert!(matches!(snapshot, Err(Error::Ineligible)));
}

#[test]
fn source_namespace_is_part_of_the_grant() {
    let wrong_source = ArtifactRevision {
        source: "other-installation".into(),
        ..binding()
    };
    assert!(matches!(
        snapshot_bytes(&provider(), &principal("good-key"), &wrong_source),
        Err(Error::SourceUnavailable)
    ));
}
