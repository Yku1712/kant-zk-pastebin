// SPDX-License-Identifier: Apache-2.0
//! Experimental authorized Artifact MCP bridge. NOT PRODUCTION-READY.
//!
//! This crate supplies a typed `zos_int07_core::AuthorizedSource` implementation. It does not
//! authenticate users itself and it never accepts publication authority from raw artifact
//! metadata, visibility flags, share tokens, or caller-supplied revision descriptions. The
//! trusted source boundary must issue a read grant and a separate public-release grant for one
//! exact, internally consistent revision snapshot.
//!
//! `ContractCompatibleProvider` is a test provider that models the observed Artifact MCP
//! policy shape. It is not an Artifact MCP client and it is not evidence of live integration.

#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use zos_int07_core::{
    ArtifactRevision, AuthorizedSnapshot, AuthorizedSource, BodyKind, ContentDigest, Error, Release,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Reader,
    Collaborator,
    Author,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Authentication {
    ApiKey,
    OAuth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Classification {
    Public,
    Private,
    Unclassified,
}

/// A server-issued identity. Fields are private so downstream adapters cannot forge one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedPrincipal {
    org: String,
    subject: String,
    role: Role,
    authentication: Authentication,
}

impl AuthenticatedPrincipal {
    fn binding(&self) -> PrincipalBinding {
        PrincipalBinding {
            org: self.org.clone(),
            subject: self.subject.clone(),
            role: self.role,
            authentication: self.authentication,
        }
    }

    pub fn is_admin(&self) -> bool {
        self.org == "admin"
    }

    pub fn org(&self) -> &str {
        &self.org
    }

    pub fn role(&self) -> Role {
        self.role
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrincipalBinding {
    org: String,
    subject: String,
    role: Role,
    authentication: Authentication,
}

/// Server-side credential facts. In a real deployment these come from the API-key table or
/// validated OAuth claims, never from the publication caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialRecord {
    org: String,
    subject: String,
    role: Role,
    authentication: Authentication,
}

impl CredentialRecord {
    pub fn api_key(org: impl Into<String>, client_id: impl Into<String>, role: Role) -> Self {
        Self {
            org: org.into(),
            subject: client_id.into(),
            role,
            authentication: Authentication::ApiKey,
        }
    }

    pub fn oauth(org: impl Into<String>, subject: impl Into<String>, role: Role) -> Self {
        Self {
            org: org.into(),
            subject: subject.into(),
            role,
            authentication: Authentication::OAuth,
        }
    }
}

/// Contract-compatible credential verifier. It models the trusted authentication boundary;
/// it performs no network authentication.
#[derive(Clone, Debug, Default)]
pub struct CredentialVerifier {
    credentials: HashMap<String, CredentialRecord>,
}

impl CredentialVerifier {
    pub fn new<I, K>(entries: I) -> Self
    where
        I: IntoIterator<Item = (K, CredentialRecord)>,
        K: Into<String>,
    {
        Self {
            credentials: entries
                .into_iter()
                .map(|(credential, record)| (credential.into(), record))
                .collect(),
        }
    }

    pub fn authenticate(&self, credential: &str) -> Result<AuthenticatedPrincipal, Error> {
        let record = self
            .credentials
            .get(credential)
            .ok_or(Error::AccessDenied)?;
        Ok(AuthenticatedPrincipal {
            org: record.org.clone(),
            subject: record.subject.clone(),
            role: record.role,
            authentication: record.authentication,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GrantBinding {
    source: String,
    tenant: String,
    artifact: String,
    revision: u64,
    digest: [u8; 32],
}

impl GrantBinding {
    fn from_revision(revision: &ArtifactRevision) -> Self {
        Self {
            source: revision.source.clone(),
            tenant: revision.tenant.clone(),
            artifact: revision.artifact.clone(),
            revision: revision.revision,
            digest: revision.digest.0,
        }
    }
}

/// Read authorization minted by the trusted source policy. Not constructible downstream.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ReadGrant {
    principal: PrincipalBinding,
    binding: GrantBinding,
    epoch: u64,
}

impl ReadGrant {
    fn validate(
        &self,
        principal: &PrincipalBinding,
        binding: &GrantBinding,
        current_epoch: u64,
    ) -> Result<(), Error> {
        if self.principal != *principal || self.binding != *binding || self.epoch != current_epoch {
            return Err(Error::AccessDenied);
        }
        Ok(())
    }
}

/// Public-release authorization. It is deliberately a different capability from `ReadGrant`:
/// ordinary artifact read access cannot satisfy it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PublicReleaseGrant {
    read: ReadGrant,
    binding: GrantBinding,
    classification: Classification,
    epoch: u64,
}

impl PublicReleaseGrant {
    fn validate(
        &self,
        read: &ReadGrant,
        binding: &GrantBinding,
        classification: Classification,
        current_epoch: u64,
    ) -> Result<(), Error> {
        if self.read != *read
            || self.binding != *binding
            || self.classification != Classification::Public
            || classification != Classification::Public
            || self.epoch != current_epoch
        {
            return Err(Error::Ineligible);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ArtifactMetaView {
    binding: GrantBinding,
    classification: Classification,
    kind: BodyKind,
}

/// Source facts for one exact revision. Fields are private; only the trusted provider module
/// can issue one.
#[derive(Clone, Debug)]
pub struct AuthoritySnapshot {
    binding: ArtifactRevision,
    body: Vec<u8>,
    principal: PrincipalBinding,
    classification: Classification,
    kind: BodyKind,
    read_grant: ReadGrant,
    release_grant: PublicReleaseGrant,
    current_epoch: u64,
    revoked: bool,
    consistency: [u8; 32],
}

impl AuthoritySnapshot {
    #[allow(clippy::too_many_arguments)]
    fn issue(
        binding: ArtifactRevision,
        body: Vec<u8>,
        principal: PrincipalBinding,
        meta: &ArtifactMetaView,
        read_grant: ReadGrant,
        release_grant: PublicReleaseGrant,
        current_epoch: u64,
        revoked: bool,
    ) -> Self {
        let mut snapshot = Self {
            binding,
            body,
            principal,
            classification: meta.classification,
            kind: meta.kind,
            read_grant,
            release_grant,
            current_epoch,
            revoked,
            consistency: [0; 32],
        };
        snapshot.consistency = snapshot.token();
        snapshot
    }

    fn token(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"zos-int08/authority-snapshot/v1\0");
        update_str(&mut hash, &self.binding.source);
        update_str(&mut hash, &self.binding.tenant);
        update_str(&mut hash, &self.binding.artifact);
        hash.update(self.binding.revision.to_be_bytes());
        hash.update(self.binding.digest.0);
        hash.update(&self.body);
        update_str(&mut hash, &self.principal.org);
        update_str(&mut hash, &self.principal.subject);
        hash.update([
            self.principal.role as u8,
            self.principal.authentication as u8,
        ]);
        hash.update([self.classification as u8, self.kind as u8]);
        hash.update(self.read_grant.epoch.to_be_bytes());
        hash.update(self.release_grant.epoch.to_be_bytes());
        hash.update(self.current_epoch.to_be_bytes());
        hash.update([u8::from(self.revoked)]);
        hash.finalize().into()
    }

    fn validate(
        &self,
        principal: &AuthenticatedPrincipal,
        requested: &ArtifactRevision,
    ) -> Result<(), Error> {
        if self.revoked {
            return Err(Error::AccessDenied);
        }
        if self.binding != *requested {
            return Err(Error::InvalidBinding);
        }
        if self.principal != principal.binding() {
            return Err(Error::AccessDenied);
        }
        if self.consistency != self.token() {
            return Err(Error::InvalidBinding);
        }
        let grant_binding = GrantBinding::from_revision(requested);
        self.read_grant
            .validate(&self.principal, &grant_binding, self.current_epoch)?;
        self.release_grant.validate(
            &self.read_grant,
            &grant_binding,
            self.classification,
            self.current_epoch,
        )?;
        if self.kind != BodyKind::SingleFile {
            return Err(Error::UnsupportedBody);
        }
        if ContentDigest::of(&self.body) != requested.digest {
            return Err(Error::DigestMismatch);
        }
        Ok(())
    }
}

fn update_str(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value.as_bytes());
}

/// Trusted-source port. An implementation must retrieve metadata and body from one coherent
/// revision view and mint both grants itself.
pub trait AuthoritativeSource {
    fn snapshot(
        &self,
        principal: &AuthenticatedPrincipal,
        requested: &ArtifactRevision,
    ) -> Result<AuthoritySnapshot, Error>;
}

/// Implements the INT07 source contract using authoritative grants and a consistent snapshot.
pub struct ArtifactMcpBridge<S: AuthoritativeSource> {
    source: S,
}

impl<S: AuthoritativeSource> ArtifactMcpBridge<S> {
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S: AuthoritativeSource> AuthorizedSource for ArtifactMcpBridge<S> {
    type Principal = AuthenticatedPrincipal;

    fn with_revision<T>(
        &self,
        principal: &AuthenticatedPrincipal,
        requested: &ArtifactRevision,
        use_snapshot: impl FnOnce(AuthorizedSnapshot<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let snapshot = self.source.snapshot(principal, requested)?;
        snapshot.validate(principal, requested)?;
        use_snapshot(AuthorizedSnapshot {
            binding: requested,
            release: Release::ExplicitlyPublic,
            kind: BodyKind::SingleFile,
            bytes: &snapshot.body,
        })
    }
}

/// Access-policy model matching the observed Artifact MCP authorization shape. This code belongs
/// to the trusted source side in tests; it is not reachable from publication callers.
#[derive(Clone, Copy, Debug, Default)]
pub struct AccessPolicy;

impl AccessPolicy {
    pub(crate) fn authorize_publisher_read(
        &self,
        principal: &AuthenticatedPrincipal,
        meta: &ArtifactMetaView,
        epoch: u64,
    ) -> Result<ReadGrant, Error> {
        let allowed = principal.is_admin() || principal.org == meta.binding.tenant;
        if !allowed {
            return Err(Error::AccessDenied);
        }
        Ok(ReadGrant {
            principal: principal.binding(),
            binding: meta.binding.clone(),
            epoch,
        })
    }

    pub(crate) fn publisher_can_publish(&self, principal: &AuthenticatedPrincipal) -> bool {
        principal.is_admin() || principal.role != Role::Reader
    }
}

/// Distinct release authority. It never derives release permission from a share token, a
/// visibility flag, or caller-provided metadata.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReleaseAuthority;

impl ReleaseAuthority {
    pub(crate) fn authorize_public_release(
        &self,
        policy: &AccessPolicy,
        principal: &AuthenticatedPrincipal,
        meta: &ArtifactMetaView,
        read: &ReadGrant,
        epoch: u64,
    ) -> Result<PublicReleaseGrant, Error> {
        if meta.classification != Classification::Public || !policy.publisher_can_publish(principal)
        {
            return Err(Error::Ineligible);
        }
        Ok(PublicReleaseGrant {
            read: read.clone(),
            binding: meta.binding.clone(),
            classification: meta.classification,
            epoch,
        })
    }
}

/// One artifact record used by the contract-compatible test provider.
#[derive(Clone, Debug)]
pub struct ArtifactRecord {
    pub binding: ArtifactRevision,
    pub classification: Classification,
    pub kind: BodyKind,
    pub body: Vec<u8>,
}

impl ArtifactRecord {
    pub fn new(
        binding: ArtifactRevision,
        classification: Classification,
        kind: BodyKind,
        body: Vec<u8>,
    ) -> Self {
        Self {
            binding,
            classification,
            kind,
            body,
        }
    }

    fn key(&self) -> (String, String, String, u64) {
        (
            self.binding.source.clone(),
            self.binding.tenant.clone(),
            self.binding.artifact.clone(),
            self.binding.revision,
        )
    }
}

/// Contract-compatible, in-memory Artifact MCP policy provider for adversarial tests.
/// It is explicitly not an Artifact MCP client and performs no live service access.
#[derive(Clone, Debug, Default)]
pub struct ContractCompatibleProvider {
    records: HashMap<(String, String, String, u64), ArtifactRecord>,
    current_epoch: u64,
    revoked: bool,
    corrupt_snapshot: bool,
    substitute_revision: Option<u64>,
    stale_grant: bool,
}

impl ContractCompatibleProvider {
    pub fn empty() -> Self {
        Self {
            current_epoch: 1,
            ..Self::default()
        }
    }

    pub fn with_record(mut self, record: ArtifactRecord) -> Self {
        self.records.insert(record.key(), record);
        self
    }

    pub fn set_epoch(&mut self, epoch: u64) {
        self.current_epoch = epoch;
    }

    pub fn epoch(&self) -> u64 {
        self.current_epoch
    }

    pub fn set_revoked(&mut self, revoked: bool) {
        self.revoked = revoked;
    }

    pub fn set_corrupt_snapshot(&mut self, corrupt: bool) {
        self.corrupt_snapshot = corrupt;
    }

    pub fn set_substitute_revision(&mut self, revision: Option<u64>) {
        self.substitute_revision = revision;
    }

    pub fn set_stale_grant(&mut self, stale: bool) {
        self.stale_grant = stale;
    }

    pub fn set_classification(
        &mut self,
        binding: &ArtifactRevision,
        classification: Classification,
    ) {
        let key = (
            binding.source.clone(),
            binding.tenant.clone(),
            binding.artifact.clone(),
            binding.revision,
        );
        if let Some(record) = self.records.get_mut(&key) {
            record.classification = classification;
        }
    }
}

impl AuthoritativeSource for ContractCompatibleProvider {
    fn snapshot(
        &self,
        principal: &AuthenticatedPrincipal,
        requested: &ArtifactRevision,
    ) -> Result<AuthoritySnapshot, Error> {
        let key = (
            requested.source.clone(),
            requested.tenant.clone(),
            requested.artifact.clone(),
            requested.revision,
        );
        let record = self.records.get(&key).ok_or(Error::SourceUnavailable)?;

        let meta = ArtifactMetaView {
            binding: GrantBinding::from_revision(&record.binding),
            classification: record.classification,
            kind: record.kind,
        };
        let grant_epoch = if self.stale_grant {
            self.current_epoch.saturating_sub(1)
        } else {
            self.current_epoch
        };
        let policy = AccessPolicy;
        let read = policy.authorize_publisher_read(principal, &meta, grant_epoch)?;
        let release = ReleaseAuthority.authorize_public_release(
            &policy,
            principal,
            &meta,
            &read,
            grant_epoch,
        )?;

        let binding = match self.substitute_revision {
            Some(revision) => ArtifactRevision {
                revision,
                ..record.binding.clone()
            },
            None => record.binding.clone(),
        };
        let mut snapshot = AuthoritySnapshot::issue(
            binding,
            record.body.clone(),
            principal.binding(),
            &meta,
            read,
            release,
            self.current_epoch,
            self.revoked,
        );
        if self.corrupt_snapshot {
            snapshot.consistency = [0; 32];
        }
        Ok(snapshot)
    }
}
