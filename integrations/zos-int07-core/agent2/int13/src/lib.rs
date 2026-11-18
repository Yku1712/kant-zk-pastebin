// SPDX-License-Identifier: Apache-2.0
//! Isolated INT07/08/10/11/12 composition. NOT PRODUCTION-READY.
#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]

use std::{
    cell::Cell,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};
use zos_int07_core::{
    AuthorizedSource, Destination, DurableRetry, Error, EvidenceOutput, Intent, Journal,
    MAX_PUBLICATION_BYTES, OperationState, PublicationEligibility, Publisher, ReceiptCandidate,
    ReceiptEvidence, Recovery,
};
use zos_ipfs_local_durability::{Error as StoreError, LocalReceipt, Store};
use zos_unixfs_readback::{
    BlockSource, Cid, Error as VerifyError, Flatfs, Limits, UnixfsReceiptEvidence, verify,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Staged,
    BeforeChildRead,
    ProviderCommitted,
}

type Observer<'a> = dyn Fn(Phase, &Path, &Cid) -> Result<(), Error> + Sync + 'a;

/// An isolated durable local destination. The probe must be the separately built INT11
/// writer with the pinned fail-closed patch applied. Repository paths are trusted.
pub struct LocalDestination<'a> {
    id: String,
    probe: PathBuf,
    repo: PathBuf,
    staging_parent: PathBuf,
    store: Store,
    observe: &'a Observer<'a>,
}

impl LocalDestination<'static> {
    pub fn open(
        id: impl Into<String>,
        probe: impl AsRef<Path>,
        repo: impl AsRef<Path>,
        staging_parent: impl AsRef<Path>,
    ) -> Result<Self, Error> {
        Self::open_observed(id, probe, repo, staging_parent, &|_, _, _| Ok(()))
    }
}

impl<'a> LocalDestination<'a> {
    fn open_observed(
        id: impl Into<String>,
        probe: impl AsRef<Path>,
        repo: impl AsRef<Path>,
        staging_parent: impl AsRef<Path>,
        observe: &'a Observer<'a>,
    ) -> Result<Self, Error> {
        let id = id.into();
        let repo = repo
            .as_ref()
            .canonicalize()
            .map_err(|_| Error::ExternalUnavailable)?;
        let staging_parent = staging_parent
            .as_ref()
            .canonicalize()
            .map_err(|_| Error::ExternalUnavailable)?;
        if repo.starts_with(&staging_parent) || staging_parent.starts_with(&repo) {
            return Err(Error::InvalidBinding);
        }
        Ok(Self {
            store: Store::open(&repo, &id).map_err(store_error)?,
            id,
            probe: probe
                .as_ref()
                .canonicalize()
                .map_err(|_| Error::ExternalUnavailable)?,
            repo,
            staging_parent,
            observe,
        })
    }

    /// Exercise the real INT07 publisher and revalidate any returned confirmation against
    /// the current destination. A cached bridge receipt alone cannot prove current readback.
    pub fn execute<S: AuthorizedSource>(
        &self,
        journal: &mut Journal,
        source: &S,
        principal: &S::Principal,
        eligibility: &impl PublicationEligibility,
        intent: &Intent,
        recovery: Recovery,
    ) -> Result<OperationState, Error> {
        let evidence = UnixfsReceiptEvidence {
            original: LocalEvidence(self),
            blocks: Flatfs::open(&self.repo).map_err(verification_error)?,
            limits: Limits::default(),
        };
        let publisher = Publisher {
            source,
            eligibility,
            destination: self,
            evidence: &evidence,
        };
        let state = publisher.execute(journal, principal, intent, recovery)?;
        if let OperationState::Confirmed(confirmed) = &state {
            let observed = self.lookup(intent)?.ok_or(Error::ExternalUnavailable)?;
            let valid_root = confirmed.ipfs().is_some_and(|value| {
                observed.ipfs_cid.as_deref() == Some(value.cid().to_string().as_str())
            });
            if observed.remote_id != confirmed.remote_id() || !valid_root {
                return Err(Error::InvalidReceipt);
            }
            if !eligibility.permits(intent)? {
                return Err(Error::Ineligible);
            }
        }
        Ok(state)
    }

    fn store_bytes(&self, intent: &Intent, body: &[u8]) -> Result<ReceiptCandidate, Error> {
        if intent.destination() != self.id {
            return Err(Error::InvalidBinding);
        }
        if body.len() > MAX_PUBLICATION_BYTES {
            return Err(Error::ResourceLimit);
        }
        if zos_int07_core::ContentDigest::of(body) != intent.artifact().digest {
            return Err(Error::DigestMismatch);
        }
        if let Some(existing) = self.store.lookup(intent).map_err(store_error)? {
            return Ok(candidate(&existing));
        }

        let stage = tempfile::Builder::new()
            .prefix("int13-")
            .tempdir_in(&self.staging_parent)
            .map_err(|_| Error::ExternalUnavailable)?;
        let input = stage.path().join("artifact");
        let blocks = stage.path().join("repo");
        fs::write(&input, body).map_err(|_| Error::ExternalUnavailable)?;
        fs::create_dir(&blocks).map_err(|_| Error::ExternalUnavailable)?;
        let output = Command::new(&self.probe)
            .env("IPFS_PATH", &blocks)
            .arg("add")
            .arg(&input)
            .output()
            .map_err(|_| Error::ExternalUnavailable)?;
        if !output.status.success() {
            return Err(Error::ExternalUnavailable);
        }
        let encoded = std::str::from_utf8(&output.stdout)
            .map_err(|_| Error::InvalidReceipt)?
            .trim();
        if encoded == "NONE" {
            return Err(Error::ExternalUnavailable);
        }
        if encoded.len() > 256 {
            return Err(Error::InvalidReceipt);
        }
        let root: Cid = encoded.parse().map_err(|_| Error::InvalidReceipt)?;
        if encoded != root.to_string() {
            return Err(Error::InvalidReceipt);
        }
        (self.observe)(Phase::Staged, &blocks, &root)?;
        let source = ObservedStaging {
            flatfs: Flatfs::open(&blocks).map_err(verification_error)?,
            path: &blocks,
            root: &root,
            observer: self.observe,
            visited_root: Cell::new(false),
        };
        let complete = self
            .store
            .commit(intent, &root, body.len(), &source)
            .map_err(store_error)?;
        (self.observe)(Phase::ProviderCommitted, &blocks, &root)?;
        Ok(candidate(&complete))
    }
}

impl Destination for LocalDestination<'_> {
    fn id(&self) -> &str {
        &self.id
    }

    fn submit(&self, intent: &Intent, body: &[u8]) -> Result<ReceiptCandidate, Error> {
        self.store_bytes(intent, body)
    }

    fn lookup(&self, intent: &Intent) -> Result<Option<ReceiptCandidate>, Error> {
        self.store
            .lookup(intent)
            .map(|result| result.as_ref().map(candidate))
            .map_err(store_error)
    }

    fn durable_retry(&self) -> Option<&dyn DurableRetry> {
        Some(self)
    }
}

impl DurableRetry for LocalDestination<'_> {
    fn retry_idempotently(
        &self,
        intent: &Intent,
        body: &[u8],
    ) -> Result<Option<ReceiptCandidate>, Error> {
        self.store_bytes(intent, body).map(Some)
    }
}

struct ObservedStaging<'a> {
    flatfs: Flatfs,
    path: &'a Path,
    root: &'a Cid,
    observer: &'a Observer<'a>,
    visited_root: Cell<bool>,
}

impl BlockSource for ObservedStaging<'_> {
    fn read_block(
        &self,
        cid: &Cid,
        limit: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>, VerifyError> {
        if self.visited_root.replace(true) && cid != self.root {
            (self.observer)(Phase::BeforeChildRead, self.path, self.root)
                .map_err(|error| VerifyError::Io(io::Error::other(error)))?;
        }
        self.flatfs.read_block(cid, limit, deadline)
    }
}

struct LocalEvidence<'a, 'b>(&'a LocalDestination<'b>);

impl ReceiptEvidence for LocalEvidence<'_, '_> {
    fn published_bytes(
        &self,
        intent: &Intent,
        supplied: &ReceiptCandidate,
        out: &mut dyn EvidenceOutput,
    ) -> Result<(), Error> {
        let saved = self
            .0
            .store
            .lookup(intent)
            .map_err(store_error)?
            .ok_or(Error::InvalidReceipt)?;
        let actual = candidate(&saved);
        if actual.operation != supplied.operation
            || actual.remote_id != supplied.remote_id
            || actual.witness != supplied.witness
            || actual.ipfs_cid != supplied.ipfs_cid
            || supplied.local_alias.is_some()
        {
            return Err(Error::InvalidReceipt);
        }
        let limits = Limits {
            output_bytes: out.remaining(),
            ..Limits::default()
        };
        verify(
            &Flatfs::open(&self.0.repo).map_err(verification_error)?,
            &saved.root,
            saved.digest.0,
            limits,
            |chunk| out.append(chunk).map_err(io::Error::other),
        )
        .map_err(verification_error)?;
        Ok(())
    }

    fn verified_ipfs_bytes(
        &self,
        _: &Intent,
        _: &Cid,
        _: &mut dyn EvidenceOutput,
    ) -> Result<(), Error> {
        Err(Error::InvalidReceipt)
    }
}

fn candidate(receipt: &LocalReceipt) -> ReceiptCandidate {
    ReceiptCandidate {
        operation: receipt.operation,
        remote_id: format!("local-{}", receipt.operation),
        witness: receipt.digest,
        local_alias: None,
        ipfs_cid: Some(receipt.root.to_string()),
    }
}

fn store_error(error: StoreError) -> Error {
    match error {
        StoreError::Conflict => Error::JournalConflict,
        StoreError::SizeLimit => Error::ResourceLimit,
        StoreError::SizeMismatch => Error::DigestMismatch,
        StoreError::Verification(error) => verification_error(error),
        _ => Error::ExternalUnavailable,
    }
}

fn verification_error(error: VerifyError) -> Error {
    match error {
        VerifyError::MissingBlock(_) | VerifyError::Io(_) | VerifyError::Deadline => {
            Error::ExternalUnavailable
        }
        VerifyError::DigestMismatch => Error::DigestMismatch,
        VerifyError::Limit(_) | VerifyError::Output(_) => Error::ResourceLimit,
        _ => Error::InvalidReceipt,
    }
}

#[cfg(test)]
mod tests;
