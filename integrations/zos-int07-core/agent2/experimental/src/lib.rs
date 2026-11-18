// SPDX-License-Identifier: Apache-2.0
//! Local reference transaction and readback for review. NOT PRODUCTION-READY.
#![forbid(unsafe_code)]

use cid::Cid;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use zos_int07_core::{
    ContentDigest, Destination, Error, Intent, ReceiptCandidate, ReceiptEvidence,
};

pub const MAX_BYTES: usize = 1024 * 1024;
const APP_ID: i64 = 0x5a4f_4432;

pub struct LocalDestination {
    path: PathBuf,
    id: String,
}

impl LocalDestination {
    /// Open a dedicated experimental database under a trusted, private directory.
    /// The database binds to the caller-supplied unique fixture instance ID.
    pub fn open(path: impl AsRef<Path>, instance: &str) -> Result<Self, Error> {
        if instance.is_empty()
            || instance.len() > 64
            || !instance
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::InvalidBinding);
        }
        let destination = Self {
            path: path.as_ref().to_owned(),
            id: format!("fixture:{instance}"),
        };
        let mut conn = destination
            .open_connection(OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match schema(&tx)? {
            (0, 0) => {
                let has_objects: bool =
                    tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema)", [], |row| {
                        row.get(0)
                    })?;
                if has_objects {
                    return Err(Error::JournalConflict);
                }
                tx.execute_batch(
                    "CREATE TABLE destination_identity (
                        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                        id TEXT NOT NULL
                    ) STRICT;
                    CREATE TABLE published_payload (
                        operation TEXT PRIMARY KEY,
                        body BLOB NOT NULL
                            CHECK(length(body) BETWEEN 1 AND 1048576)
                    ) STRICT;",
                )?;
                tx.execute(
                    "INSERT INTO destination_identity VALUES (1, ?1)",
                    [&destination.id],
                )?;
                tx.pragma_update(None, "application_id", APP_ID)?;
                tx.pragma_update(None, "user_version", 1)?;
            }
            (APP_ID, 1) => destination.check_identity(&tx)?,
            _ => return Err(Error::JournalConflict),
        }
        tx.commit()?;
        Ok(destination)
    }

    fn open_connection(&self, flags: OpenFlags) -> Result<Connection, Error> {
        let conn = Connection::open_with_flags(&self.path, flags)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        if flags.contains(OpenFlags::SQLITE_OPEN_READ_WRITE) {
            let mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
            if mode != "delete" {
                return Err(Error::JournalConflict);
            }
            conn.pragma_update(None, "synchronous", "FULL")?;
        }
        Ok(conn)
    }

    fn check_identity(&self, conn: &Connection) -> Result<(), Error> {
        let id: String = conn.query_row(
            "SELECT id FROM destination_identity WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        if id != self.id {
            return Err(Error::InvalidBinding);
        }
        Ok(())
    }

    fn validate(&self, conn: &Connection, intent: &Intent) -> Result<(), Error> {
        if intent.destination() != self.id {
            return Err(Error::InvalidBinding);
        }
        if schema(conn)? != (APP_ID, 1) {
            return Err(Error::JournalConflict);
        }
        self.check_identity(conn)
    }
}

fn schema(conn: &Connection) -> Result<(i64, i64), Error> {
    Ok((
        conn.pragma_query_value(None, "application_id", |row| row.get(0))?,
        conn.pragma_query_value(None, "user_version", |row| row.get(0))?,
    ))
}

fn read_body(conn: &Connection, operation: &str) -> Result<Option<Vec<u8>>, Error> {
    Ok(conn
        .query_row(
            "SELECT body FROM published_payload WHERE operation = ?1
                AND length(body) BETWEEN 1 AND ?2",
            params![operation, MAX_BYTES],
            |row| row.get(0),
        )
        .optional()?)
}

fn candidate(intent: &Intent) -> ReceiptCandidate {
    ReceiptCandidate {
        operation: intent.operation(),
        remote_id: format!("fixture-{}", intent.operation()),
        local_alias: None,
        witness: intent.artifact().digest,
        ipfs_cid: None,
    }
}

impl Destination for LocalDestination {
    fn id(&self) -> &str {
        &self.id
    }

    fn submit(&self, intent: &Intent, body: &[u8]) -> Result<ReceiptCandidate, Error> {
        if body.is_empty() || body.len() > MAX_BYTES {
            return Err(Error::UnsupportedBody);
        }
        if ContentDigest::of(body) != intent.artifact().digest {
            return Err(Error::DigestMismatch);
        }
        let mut conn = self.open_connection(OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.validate(&tx, intent)?;
        let operation = intent.operation().to_string();
        tx.execute(
            "INSERT INTO published_payload (operation, body) VALUES (?1, ?2)
             ON CONFLICT(operation) DO NOTHING",
            params![operation, body],
        )?;
        if read_body(&tx, &operation)?.as_deref() != Some(body) {
            return Err(Error::JournalConflict);
        }
        tx.commit()?;
        Ok(candidate(intent))
    }

    fn lookup(&self, intent: &Intent) -> Result<Option<ReceiptCandidate>, Error> {
        let conn = self.open_connection(OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        self.validate(&conn, intent)?;
        let found: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM published_payload WHERE operation = ?1)",
            [intent.operation().to_string()],
            |row| row.get(0),
        )?;
        Ok(found.then(|| candidate(intent)))
    }

    fn retry_idempotently(
        &self,
        intent: &Intent,
        body: &[u8],
    ) -> Result<Option<ReceiptCandidate>, Error> {
        self.submit(intent, body).map(Some)
    }
}

impl ReceiptEvidence for LocalDestination {
    fn published_bytes(
        &self,
        intent: &Intent,
        receipt: &ReceiptCandidate,
    ) -> Result<Vec<u8>, Error> {
        let expected = candidate(intent);
        if receipt.operation != expected.operation
            || receipt.remote_id != expected.remote_id
            || receipt.witness != expected.witness
            || receipt.local_alias.is_some()
            || receipt.ipfs_cid.is_some()
        {
            return Err(Error::InvalidReceipt);
        }
        let conn = self.open_connection(OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        self.validate(&conn, intent)?;
        let body =
            read_body(&conn, &intent.operation().to_string())?.ok_or(Error::InvalidReceipt)?;
        if ContentDigest::of(&body) != expected.witness {
            return Err(Error::DigestMismatch);
        }
        Ok(body)
    }

    fn verified_ipfs_bytes(&self, _: &Intent, _: &Cid) -> Result<Vec<u8>, Error> {
        Err(Error::InvalidReceipt)
    }
}
