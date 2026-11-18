// SPDX-License-Identifier: Apache-2.0
//! Experimental Linux local graph completion. NOT PRODUCTION-READY.
#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]

use data_encoding::BASE32_NOPAD;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    fmt,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use zos_int07_core::{ContentDigest, Intent, MAX_PUBLICATION_BYTES, OperationId};
use zos_unixfs_readback::{BlockSource, Cid, Error as VerifyError, Flatfs, Limits, verify};

const APPLICATION_ID: i64 = 0x5a49_503c;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Sql(rusqlite::Error),
    Verification(VerifyError),
    InvalidStore,
    Conflict,
    SizeLimit,
    SizeMismatch,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "storage I/O: {error}"),
            Self::Sql(error) => write!(f, "completion ledger: {error}"),
            Self::Verification(error) => write!(f, "graph verification: {error}"),
            Self::InvalidStore => f.write_str("incompatible completion store"),
            Self::Conflict => f.write_str("publication binding or receipt conflict"),
            Self::SizeLimit => f.write_str("authoritative file size exceeds the limit"),
            Self::SizeMismatch => {
                f.write_str("reconstructed file length does not match the intent")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sql(error) => Some(error),
            Self::Verification(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}
impl From<VerifyError> for Error {
    fn from(error: VerifyError) -> Self {
        Self::Verification(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalReceipt {
    pub operation: OperationId,
    pub root: Cid,
    pub digest: ContentDigest,
    pub bytes: usize,
}

#[derive(Debug, Default)]
pub struct Orphans {
    pub unreferenced_blocks: Vec<PathBuf>,
    pub abandoned_temporary_files: Vec<PathBuf>,
}

/// An exclusively managed repository and its local completion ledger.
/// Route all mutations through this store. Use a trusted, pre-provisioned local filesystem.
pub struct Store {
    path: PathBuf,
    identity: String,
}

impl Store {
    pub fn open(path: impl AsRef<Path>, identity: impl Into<String>) -> Result<Self, Error> {
        let path = path.as_ref().canonicalize()?;
        let identity = identity.into();
        if !path.is_dir() || path.parent().is_none() || identity.is_empty() || identity.len() > 512
        {
            return Err(Error::InvalidStore);
        }
        let store = Self { path, identity };
        let mut database = store
            .open_database(OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE)?;
        let tx = database.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let application: i64 = tx.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match (application, version) {
            (0, 0) => {
                let existing: bool =
                    tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema)", [], |row| {
                        row.get(0)
                    })?;
                if existing {
                    return Err(Error::InvalidStore);
                }
                tx.execute_batch(
                    "CREATE TABLE store_identity (
                         singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                         destination TEXT NOT NULL
                     ) STRICT;
                     CREATE TABLE receipts (
                         operation TEXT PRIMARY KEY,
                         source TEXT NOT NULL,
                         tenant TEXT NOT NULL,
                         artifact TEXT NOT NULL,
                         revision BLOB NOT NULL CHECK(length(revision) = 8),
                         root TEXT NOT NULL,
                         digest BLOB NOT NULL CHECK(length(digest) = 32),
                         bytes INTEGER NOT NULL CHECK(bytes >= 0),
                         UNIQUE(source, tenant, artifact, revision)
                     ) STRICT;
                     CREATE TABLE receipt_blocks (
                         operation TEXT NOT NULL REFERENCES receipts(operation),
                         key TEXT NOT NULL,
                         PRIMARY KEY(operation, key)
                     ) STRICT;",
                )?;
                tx.execute(
                    "INSERT INTO store_identity VALUES (1, ?1)",
                    [&store.identity],
                )?;
                tx.pragma_update(None, "application_id", APPLICATION_ID)?;
                tx.pragma_update(None, "user_version", 1)?;
            }
            (APPLICATION_ID, 1) => store.check_identity(&tx)?,
            _ => return Err(Error::InvalidStore),
        }
        ensure_directory(&store.path.join("blocks"))?;
        sync_dir(&store.path)?;
        sync_dir(store.path.parent().ok_or(Error::InvalidStore)?)?;
        tx.commit()?;
        Ok(store)
    }

    fn connection(&self) -> Result<Connection, Error> {
        self.open_database(OpenFlags::SQLITE_OPEN_READ_WRITE)
    }

    fn open_database(&self, flags: OpenFlags) -> Result<Connection, Error> {
        let database = Connection::open_with_flags(self.path.join("completion.sqlite"), flags)?;
        database.busy_timeout(Duration::from_secs(15))?;
        let mode: String = database.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        if mode != "delete" {
            return Err(Error::InvalidStore);
        }
        database.pragma_update(None, "synchronous", "EXTRA")?;
        database.pragma_update(None, "foreign_keys", "ON")?;
        Ok(database)
    }

    fn check_identity(&self, database: &Connection) -> Result<(), Error> {
        let saved: String = database.query_row(
            "SELECT destination FROM store_identity WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        if saved != self.identity {
            return Err(Error::InvalidStore);
        }
        Ok(())
    }

    /// Stage blocks and commit a local completion receipt after full readback verification.
    /// The caller supplies an independently authorized intent and exact original byte count.
    pub fn commit(
        &self,
        intent: &Intent,
        root: &Cid,
        bytes: usize,
        staging: &impl BlockSource,
    ) -> Result<LocalReceipt, Error> {
        self.commit_observed(intent, root, bytes, staging, |_, _| Ok(()))
    }

    fn commit_observed(
        &self,
        intent: &Intent,
        root: &Cid,
        bytes: usize,
        staging: &impl BlockSource,
        observer: impl FnMut(Phase, Option<&Cid>) -> io::Result<()>,
    ) -> Result<LocalReceipt, Error> {
        if bytes > MAX_PUBLICATION_BYTES || bytes > Limits::default().output_bytes {
            return Err(Error::SizeLimit);
        }
        self.check_intent(intent)?;
        let receipt = LocalReceipt {
            operation: intent.operation(),
            root: *root,
            digest: intent.artifact().digest,
            bytes,
        };
        let mut database = self.connection()?;
        let tx = database.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let observer = RefCell::new(observer);
        self.check_identity(&tx)?;
        if let Some(previous) = find_receipt(&tx, intent)? {
            if previous != receipt {
                return Err(Error::Conflict);
            }
            self.verify_stored(&previous)?;
            tx.commit()?;
            return Ok(previous);
        }
        let binding = intent.artifact();
        let occupied: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM receipts
             WHERE source = ?1 AND tenant = ?2 AND artifact = ?3 AND revision = ?4)",
            params![
                binding.source,
                binding.tenant,
                binding.artifact,
                binding.revision.to_be_bytes()
            ],
            |row| row.get(0),
        )?;
        if occupied {
            return Err(Error::Conflict);
        }

        let target = Flatfs::open(&self.path)?;
        let promotion = Promotion {
            target,
            blocks: self.path.join("blocks"),
            staging,
            keys: RefCell::new(BTreeSet::new()),
            observer: &observer,
        };
        verify_exact(&promotion, &receipt)?;
        sync_dir(&promotion.blocks)?;
        self.verify_stored(&receipt)?;
        save_receipt(&tx, intent, &receipt, promotion.keys.into_inner())?;
        observer.borrow_mut()(Phase::BeforeCommit, None)?;
        tx.commit()?;
        observer.borrow_mut()(Phase::AfterCommit, None)?;
        Ok(receipt)
    }

    /// Verify receipt identity and disk content again before reporting a complete local graph.
    pub fn lookup(&self, intent: &Intent) -> Result<Option<LocalReceipt>, Error> {
        self.check_intent(intent)?;
        let database = self.connection()?;
        self.check_identity(&database)?;
        let receipt = find_receipt(&database, intent)?;
        if let Some(receipt) = &receipt {
            self.verify_stored(receipt)?;
        }
        Ok(receipt)
    }

    fn check_intent(&self, intent: &Intent) -> Result<(), Error> {
        if intent.destination() != self.identity {
            return Err(Error::Conflict);
        }
        Ok(())
    }

    fn verify_stored(&self, receipt: &LocalReceipt) -> Result<(), Error> {
        verify_exact(&Flatfs::open(&self.path)?, receipt)
    }

    /// Inventory-only orphan audit under the same SQLite writer lock as promotions.
    pub fn orphans(&self) -> Result<Orphans, Error> {
        let mut database = self.connection()?;
        let tx = database.transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.check_identity(&tx)?;
        let referenced = {
            let mut statement = tx.prepare("SELECT DISTINCT key FROM receipt_blocks")?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<BTreeSet<_>, _>>()?
        };
        let mut inventory = Orphans::default();
        for shard in fs::read_dir(self.path.join("blocks"))? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            for entry in fs::read_dir(shard.path())? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if name.starts_with(".pending-") {
                    inventory.abandoned_temporary_files.push(entry.path());
                } else if let Some(key) = name.strip_suffix(".data")
                    && recognized_key(key, &shard.file_name())
                    && !referenced.contains(key)
                {
                    inventory.unreferenced_blocks.push(entry.path());
                }
            }
        }
        tx.commit()?;
        inventory.unreferenced_blocks.sort();
        inventory.abandoned_temporary_files.sort();
        Ok(inventory)
    }
}

fn recognized_key(key: &str, directory: &std::ffi::OsStr) -> bool {
    key.len() == 55
        && key.is_ascii()
        && directory == &key[key.len() - 3..key.len() - 1]
        && BASE32_NOPAD
            .decode(key.as_bytes())
            .is_ok_and(|bytes| bytes.len() == 34 && bytes.starts_with(&[0x12, 0x20]))
}

fn find_receipt(database: &Connection, intent: &Intent) -> Result<Option<LocalReceipt>, Error> {
    let operation = intent.operation();
    let row = database
        .query_row(
            "SELECT source, tenant, artifact, revision, root, digest, bytes
         FROM receipts WHERE operation = ?1",
            [operation.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, [u8; 8]>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, [u8; 32]>(5)?,
                    row.get::<_, usize>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((source, tenant, artifact, revision, root, digest, bytes)) = row else {
        return Ok(None);
    };
    let binding = intent.artifact();
    if source != binding.source
        || tenant != binding.tenant
        || artifact != binding.artifact
        || u64::from_be_bytes(revision) != binding.revision
        || digest != binding.digest.0
        || bytes > MAX_PUBLICATION_BYTES
        || root.len() > 256
    {
        return Err(Error::Conflict);
    }
    let cid: Cid = root.parse().map_err(|_| Error::Conflict)?;
    if cid.to_string() != root {
        return Err(Error::Conflict);
    }
    Ok(Some(LocalReceipt {
        operation,
        root: cid,
        digest: ContentDigest(digest),
        bytes,
    }))
}

fn save_receipt(
    tx: &Transaction<'_>,
    intent: &Intent,
    receipt: &LocalReceipt,
    keys: BTreeSet<String>,
) -> Result<(), Error> {
    let binding = intent.artifact();
    let operation = intent.operation().to_string();
    tx.execute(
        "INSERT INTO receipts (operation, source, tenant, artifact, revision, root, digest, bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            operation,
            binding.source,
            binding.tenant,
            binding.artifact,
            binding.revision.to_be_bytes(),
            receipt.root.to_string(),
            receipt.digest.0,
            receipt.bytes,
        ],
    )?;
    let mut add = tx.prepare("INSERT INTO receipt_blocks VALUES (?1, ?2)")?;
    for key in keys {
        add.execute(params![operation, key])?;
    }
    Ok(())
}

fn verify_exact(source: &impl BlockSource, receipt: &LocalReceipt) -> Result<(), Error> {
    let limits = Limits {
        output_bytes: receipt.bytes,
        ..Limits::default()
    };
    let result = verify(source, &receipt.root, receipt.digest.0, limits, |_| Ok(()))?;
    if result.bytes != receipt.bytes {
        return Err(Error::SizeMismatch);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    BeforeBlock,
    DuringTempWrite,
    AfterFileSync,
    AfterLink,
    AfterBlockSync,
    BeforeCommit,
    AfterCommit,
}

struct Promotion<'a, S, F> {
    target: Flatfs,
    blocks: PathBuf,
    staging: &'a S,
    keys: RefCell<BTreeSet<String>>,
    observer: &'a RefCell<F>,
}

impl<S: BlockSource, F: FnMut(Phase, Option<&Cid>) -> io::Result<()>> BlockSource
    for Promotion<'_, S, F>
{
    fn read_block(
        &self,
        cid: &Cid,
        limit: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>, VerifyError> {
        let bytes = self.staging.read_block(cid, limit, deadline)?;
        if bytes.len() > limit {
            return Err(VerifyError::Limit("block or encoded bytes"));
        }
        if Sha256::digest(&bytes).as_slice() != cid.hash().digest() {
            return Err(VerifyError::HashMismatch(*cid));
        }

        let key = BASE32_NOPAD.encode(&cid.hash().to_bytes());
        if !self.keys.borrow().contains(&key) {
            self.persist(cid, &key, &bytes, deadline)?;
            self.keys.borrow_mut().insert(key);
        }
        Ok(bytes)
    }
}

impl<S, F: FnMut(Phase, Option<&Cid>) -> io::Result<()>> Promotion<'_, S, F> {
    fn persist(
        &self,
        cid: &Cid,
        key: &str,
        bytes: &[u8],
        deadline: Instant,
    ) -> Result<(), VerifyError> {
        let emit = |phase| self.observer.borrow_mut()(phase, Some(cid)).map_err(VerifyError::Io);
        emit(Phase::BeforeBlock)?;
        let directory = self.blocks.join(&key[key.len() - 3..key.len() - 1]);
        ensure_directory(&directory).map_err(VerifyError::Io)?;
        let path = directory.join(format!("{key}.data"));
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                if self.target.read_block(cid, bytes.len(), deadline)? != bytes {
                    return Err(VerifyError::HashMismatch(*cid));
                }
                File::open(&path)
                    .and_then(|file| file.sync_all())
                    .map_err(VerifyError::Io)?;
            }
            Ok(_) => {
                return Err(VerifyError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "flatfs block path is not a regular file",
                )));
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(VerifyError::Io(error));
            }
            Err(_) => {
                let mut temporary = tempfile::Builder::new()
                    .prefix(".pending-")
                    .tempfile_in(&directory)
                    .map_err(VerifyError::Io)?;
                let (first, second) = bytes.split_at(bytes.len() / 2);
                temporary.write_all(first).map_err(VerifyError::Io)?;
                emit(Phase::DuringTempWrite)?;
                temporary.write_all(second).map_err(VerifyError::Io)?;
                temporary.as_file().sync_all().map_err(VerifyError::Io)?;
                emit(Phase::AfterFileSync)?;
                fs::hard_link(temporary.path(), &path).map_err(VerifyError::Io)?;
                emit(Phase::AfterLink)?;
                temporary.close().map_err(VerifyError::Io)?;
            }
        }
        sync_dir(&directory).map_err(VerifyError::Io)?;
        emit(Phase::AfterBlockSync)?;
        Ok(())
    }
}

fn ensure_directory(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error)
            if error.kind() == io::ErrorKind::AlreadyExists
                && fs::symlink_metadata(path)?.is_dir() =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(test)]
mod tests;
