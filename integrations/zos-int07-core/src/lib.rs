// SPDX-License-Identifier: Apache-2.0
//! Experimental publication contracts and SQLite journal. NOT PRODUCTION-READY.
#![forbid(unsafe_code)]

mod journal;

use cid::Cid;
pub use journal::Journal;
use sha2::{Digest, Sha256};
use std::{fmt, str::FromStr};

#[derive(Debug)]
pub enum Error {
    InvalidBinding,
    AccessDenied,
    SourceUnavailable,
    Ineligible,
    UnsupportedBody,
    DigestMismatch,
    InvalidReceipt,
    ExternalUnavailable,
    JournalConflict,
    Sqlite(rusqlite::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidBinding => "invalid publication binding",
            Self::AccessDenied => "artifact access denied",
            Self::SourceUnavailable => "artifact source unavailable",
            Self::Ineligible => "revision has no eligible public release",
            Self::UnsupportedBody => "unsupported artifact body",
            Self::DigestMismatch => "artifact or receipt digest mismatch",
            Self::InvalidReceipt => "invalid or conflicting external receipt",
            Self::ExternalUnavailable => "external publication result unavailable",
            Self::JournalConflict => "journal record or state conflicts with intent",
            Self::Sqlite(_) => "SQLite operation failed",
        };
        f.write_str(message)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentDigest(pub [u8; 32]);

impl ContentDigest {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// This is Pastebin's truncated local alias, with no IPFS or receipt semantics.
    pub fn pastebin_alias(self) -> LocalAlias {
        let hex: String = self.0[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        LocalAlias(format!("bafk{hex}"))
    }
}

impl FromStr for ContentDigest {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Error> {
        parse_sha256_hex(value).map(Self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAlias(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedIpfs(Cid);

impl VerifiedIpfs {
    pub fn cid(&self) -> &Cid {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactRevision {
    /// Stable namespace of the authenticated Artifact MCP installation.
    pub source: String,
    pub tenant: String,
    pub artifact: String,
    pub revision: u64,
    pub digest: ContentDigest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationId([u8; 32]);

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for OperationId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Error> {
        parse_sha256_hex(value).map(Self)
    }
}

fn parse_sha256_hex(value: &str) -> Result<[u8; 32], Error> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(Error::InvalidBinding);
    }
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[2 * index..2 * index + 2], 16)
            .map_err(|_| Error::InvalidBinding)?;
    }
    Ok(digest)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Intent {
    artifact: ArtifactRevision,
    destination: String,
    operation: OperationId,
}

impl Intent {
    pub fn new(artifact: ArtifactRevision, destination: String) -> Result<Self, Error> {
        let parts = [
            artifact.source.as_str(),
            artifact.tenant.as_str(),
            artifact.artifact.as_str(),
            destination.as_str(),
        ];
        if artifact.revision == 0 || parts.iter().any(|part| !valid_identifier(part)) {
            return Err(Error::InvalidBinding);
        }
        let mut hash = Sha256::new();
        hash.update(b"zos-int07/public-single-file/v1\0");
        for part in parts {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part.as_bytes());
        }
        hash.update(artifact.revision.to_be_bytes());
        hash.update(artifact.digest.0);
        Ok(Self {
            artifact,
            destination,
            operation: OperationId(hash.finalize().into()),
        })
    }

    pub fn artifact(&self) -> &ArtifactRevision {
        &self.artifact
    }

    pub fn destination(&self) -> &str {
        &self.destination
    }

    pub fn operation(&self) -> OperationId {
        self.operation
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Release {
    Private,
    Unclassified,
    ExplicitlyPublic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BodyKind {
    SingleFile,
    Bundle,
}

pub struct AuthorizedSnapshot<'a> {
    pub binding: &'a ArtifactRevision,
    pub release: Release,
    pub kind: BodyKind,
    pub bytes: &'a [u8],
}

/// Trusted source boundary. Authenticate the caller, enforce tenant access, and supply
/// authoritative revision metadata and body from one consistent snapshot. Keep release
/// authorization valid throughout the callback via a release guard or immutable release.
pub trait AuthorizedSource {
    type Principal: ?Sized;

    fn with_revision<T>(
        &self,
        principal: &Self::Principal,
        requested: &ArtifactRevision,
        use_snapshot: impl FnOnce(AuthorizedSnapshot<'_>) -> Result<T, Error>,
    ) -> Result<T, Error>;
}

/// Destination-specific public eligibility, evaluated without giving the policy body bytes.
pub trait PublicationEligibility {
    fn permits(&self, intent: &Intent) -> Result<bool, Error>;
}

/// An untrusted external observation, including the operation correlation supplied by the
/// provider or its durable receipt ledger. Response fields alone cannot verify publication.
#[derive(Clone, Debug)]
pub struct ReceiptCandidate {
    pub operation: OperationId,
    pub remote_id: String,
    pub local_alias: Option<LocalAlias>,
    pub witness: ContentDigest,
    pub ipfs_cid: Option<String>,
}

pub trait Destination {
    fn id(&self) -> &str;

    fn submit(&self, intent: &Intent, body: &[u8]) -> Result<ReceiptCandidate, Error>;

    /// Read-only reconciliation. An absent or inconclusive result stays unresolved.
    fn lookup(&self, intent: &Intent) -> Result<Option<ReceiptCandidate>, Error>;

    /// Optional reconciliation by submission. Implement only when the remote side
    /// atomically deduplicates by operation ID, including concurrent original submissions
    /// and retries after timeout. The default sends nothing.
    fn retry_idempotently(
        &self,
        _intent: &Intent,
        _body: &[u8],
    ) -> Result<Option<ReceiptCandidate>, Error> {
        Ok(None)
    }
}

/// Trusted evidence boundary. Implementations must authenticate the destination, check durable
/// correlation to the full intent, and retrieve independently observed original payload bytes.
/// HTTP success, echoed metadata, a witness, or a local alias alone are insufficient.
pub trait ReceiptEvidence {
    fn published_bytes(
        &self,
        intent: &Intent,
        receipt: &ReceiptCandidate,
    ) -> Result<Vec<u8>, Error>;

    /// Fetch and verify linked block hashes from the requested root and reconstruct the original
    /// file. Return an error for unsupported codecs, missing blocks, or unverifiable data.
    fn verified_ipfs_bytes(&self, intent: &Intent, root: &Cid) -> Result<Vec<u8>, Error>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmedReceipt {
    remote_id: String,
    ipfs: Option<VerifiedIpfs>,
}

impl ConfirmedReceipt {
    pub fn remote_id(&self) -> &str {
        &self.remote_id
    }

    pub fn ipfs(&self) -> Option<&VerifiedIpfs> {
        self.ipfs.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationState {
    Prepared,
    Unresolved,
    Confirmed(ConfirmedReceipt),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Recovery {
    #[default]
    LookupOnly,
    RetryIdempotently,
}

pub struct Publisher<'a, S, P, D, V> {
    pub source: &'a S,
    pub eligibility: &'a P,
    pub destination: &'a D,
    pub evidence: &'a V,
}

impl<S: AuthorizedSource, P: PublicationEligibility, D: Destination, V: ReceiptEvidence>
    Publisher<'_, S, P, D, V>
{
    /// Reauthorize before every attempt or replay. External work occurs only after the
    /// journal commits its unresolved state, and outside every SQLite transaction.
    pub fn execute(
        &self,
        journal: &mut Journal,
        principal: &S::Principal,
        intent: &Intent,
        recovery: Recovery,
    ) -> Result<OperationState, Error> {
        if self.destination.id() != intent.destination() {
            return Err(Error::InvalidBinding);
        }
        self.source
            .with_revision(principal, intent.artifact(), |snapshot| {
                if snapshot.binding != intent.artifact() {
                    return Err(Error::InvalidBinding);
                }
                if snapshot.release != Release::ExplicitlyPublic
                    || !self.eligibility.permits(intent)?
                {
                    return Err(Error::Ineligible);
                }
                if snapshot.kind != BodyKind::SingleFile {
                    return Err(Error::UnsupportedBody);
                }
                if ContentDigest::of(snapshot.bytes) != intent.artifact.digest {
                    return Err(Error::DigestMismatch);
                }

                journal.prepare(intent)?;
                let candidate = match journal.claim_or_read(intent)? {
                    journal::Claim::FirstDispatch => {
                        self.destination.submit(intent, snapshot.bytes)?
                    }
                    journal::Claim::Existing(OperationState::Unresolved) => {
                        let found = self.destination.lookup(intent)?;
                        let found = match (found, recovery) {
                            (None, Recovery::RetryIdempotently) => self
                                .destination
                                .retry_idempotently(intent, snapshot.bytes)?,
                            (found, _) => found,
                        };
                        let Some(receipt) = found else {
                            return journal.state(intent)?.ok_or(Error::JournalConflict);
                        };
                        receipt
                    }
                    journal::Claim::Existing(state @ OperationState::Confirmed(_)) => {
                        return Ok(state);
                    }
                    journal::Claim::Existing(OperationState::Prepared) => {
                        return Err(Error::JournalConflict);
                    }
                };
                let receipt = self.verify(intent, &candidate)?;
                journal.confirm(intent, &receipt)?;
                Ok(OperationState::Confirmed(receipt))
            })
    }

    fn verify(
        &self,
        intent: &Intent,
        candidate: &ReceiptCandidate,
    ) -> Result<ConfirmedReceipt, Error> {
        if candidate.operation != intent.operation
            || candidate.witness != intent.artifact.digest
            || !valid_identifier(&candidate.remote_id)
            || candidate
                .local_alias
                .as_ref()
                .is_some_and(|alias| *alias != candidate.witness.pastebin_alias())
        {
            return Err(Error::InvalidReceipt);
        }
        let ipfs = candidate
            .ipfs_cid
            .as_ref()
            .map(|value| value.parse::<Cid>().map_err(|_| Error::InvalidReceipt))
            .transpose()?;
        let published = self.evidence.published_bytes(intent, candidate)?;
        if ContentDigest::of(&published) != intent.artifact.digest {
            return Err(Error::DigestMismatch);
        }
        if let Some(root) = &ipfs {
            if !matches!(root.codec(), 0x55 | 0x70)
                || root.hash().code() != 0x12
                || root.hash().size() != 32
            {
                return Err(Error::InvalidReceipt);
            }
            let bytes = self.evidence.verified_ipfs_bytes(intent, root)?;
            let digest = ContentDigest::of(&bytes);
            if digest != intent.artifact.digest {
                return Err(Error::DigestMismatch);
            }
            // Raw blocks can also be verified locally. DAG roots require verified traversal
            // by the evidence provider because the root hashes a block, not the whole file.
            if root.codec() == 0x55 && root.hash().digest() != digest.0 {
                return Err(Error::InvalidReceipt);
            }
        }
        Ok(ConfirmedReceipt {
            remote_id: candidate.remote_id.clone(),
            ipfs: ipfs.map(VerifiedIpfs),
        })
    }
}
