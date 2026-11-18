// SPDX-License-Identifier: Apache-2.0

use crate::{
    ArtifactRevision, ConfirmedReceipt, ContentDigest, Error, Intent, MAX_CID_BYTES,
    MAX_IDENTIFIER_BYTES, OperationState, VerifiedIpfs, valid_identifier,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{path::Path, time::Duration};

const APP_ID: i64 = 0x5a4f_5337;

pub struct Journal {
    connection: Connection,
}

pub(crate) enum Claim {
    FirstDispatch,
    Existing(OperationState),
}

impl Journal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        // Use SQLite's persistent rollback journal with FULL synchronization. Refuse
        // memory-only modes so an accidentally opened in-memory DB fails closed.
        let mode: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        if mode != "delete" {
            return Err(Error::JournalConflict);
        }
        connection.pragma_update(None, "synchronous", "FULL")?;

        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let application: i64 = tx.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match (application, version) {
            (0, 0) => {
                let has_objects: bool =
                    tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema)", [], |row| {
                        row.get(0)
                    })?;
                if has_objects {
                    return Err(Error::JournalConflict);
                }
                tx.execute_batch(
                    "CREATE TABLE publication (
                        operation BLOB PRIMARY KEY CHECK(length(operation) = 32),
                        source TEXT NOT NULL,
                        tenant TEXT NOT NULL,
                        artifact TEXT NOT NULL,
                        revision BLOB NOT NULL CHECK(length(revision) = 8),
                        digest BLOB NOT NULL CHECK(length(digest) = 32),
                        destination TEXT NOT NULL,
                        state TEXT NOT NULL CHECK(state IN ('PREPARED', 'UNRESOLVED', 'CONFIRMED')),
                        remote_id TEXT,
                        ipfs_cid TEXT,
                        UNIQUE (source, tenant, artifact, revision, destination),
                        CHECK((state = 'CONFIRMED' AND remote_id IS NOT NULL)
                             OR (state != 'CONFIRMED' AND remote_id IS NULL AND ipfs_cid IS NULL))
                    ) STRICT;",
                )?;
                tx.pragma_update(None, "application_id", APP_ID)?;
                tx.pragma_update(None, "user_version", 1)?;
            }
            (APP_ID, 1) => {}
            _ => return Err(Error::JournalConflict),
        }
        tx.commit()?;
        Ok(Self { connection })
    }

    pub fn state(&self, intent: &Intent) -> Result<Option<OperationState>, Error> {
        read_state(&self.connection, intent)
    }

    /// Return persisted work identifiers only. Callers must reauthorize before executing them.
    pub fn pending(&self) -> Result<Vec<Intent>, Error> {
        let mut statement = self.connection.prepare(
            "SELECT operation, source, tenant, artifact, revision, digest, destination
             FROM publication WHERE state IN ('PREPARED', 'UNRESOLVED') ORDER BY operation",
        )?;
        let mut rows = statement.query([])?;
        let mut intents = Vec::new();
        while let Some(row) = rows.next()? {
            let intent = Intent::new(
                ArtifactRevision {
                    source: row.get(1)?,
                    tenant: row.get(2)?,
                    artifact: row.get(3)?,
                    revision: u64::from_be_bytes(row.get(4)?),
                    digest: ContentDigest(row.get(5)?),
                },
                row.get(6)?,
            )?;
            if row.get::<_, [u8; 32]>(0)? != intent.operation.0 {
                return Err(Error::JournalConflict);
            }
            intents.push(intent);
        }
        Ok(intents)
    }

    pub(crate) fn prepare(&mut self, intent: &Intent) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO publication
                (operation, source, tenant, artifact, revision, digest, destination, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'PREPARED')
             ON CONFLICT DO NOTHING",
            params![
                intent.operation.0,
                intent.artifact.source,
                intent.artifact.tenant,
                intent.artifact.artifact,
                intent.artifact.revision.to_be_bytes(),
                intent.artifact.digest.0,
                intent.destination,
            ],
        )?;
        read_state(&tx, intent)?.ok_or(Error::JournalConflict)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn claim_or_read(&mut self, intent: &Intent) -> Result<Claim, Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = read_state(&tx, intent)?.ok_or(Error::JournalConflict)?;
        let outcome = match state {
            OperationState::Prepared => {
                tx.execute(
                    "UPDATE publication SET state = 'UNRESOLVED' WHERE operation = ?1",
                    [intent.operation.0],
                )?;
                Claim::FirstDispatch
            }
            state => Claim::Existing(state),
        };
        tx.commit()?;
        Ok(outcome)
    }

    pub(crate) fn confirm(
        &mut self,
        intent: &Intent,
        receipt: &ConfirmedReceipt,
    ) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        match read_state(&tx, intent)? {
            Some(OperationState::Unresolved) => {
                tx.execute(
                    "UPDATE publication SET state = 'CONFIRMED', remote_id = ?2, ipfs_cid = ?3
                     WHERE operation = ?1",
                    params![
                        intent.operation.0,
                        receipt.remote_id,
                        receipt.ipfs.as_ref().map(|value| value.cid().to_string()),
                    ],
                )?;
            }
            Some(OperationState::Confirmed(existing)) if &existing == receipt => {}
            _ => return Err(Error::JournalConflict),
        }
        tx.commit()?;
        Ok(())
    }
}

fn read_state(connection: &Connection, intent: &Intent) -> Result<Option<OperationState>, Error> {
    let record = connection
        .query_row(
            "SELECT source, tenant, artifact, revision, digest, destination, state,
                    CASE WHEN length(CAST(remote_id AS BLOB)) > ?2 THEN '' ELSE remote_id END,
                    CASE WHEN length(CAST(ipfs_cid AS BLOB)) > ?3 THEN '' ELSE ipfs_cid END
             FROM publication WHERE operation = ?1",
            params![intent.operation.0, MAX_IDENTIFIER_BYTES, MAX_CID_BYTES],
            |row| {
                Ok((
                    ArtifactRevision {
                        source: row.get(0)?,
                        tenant: row.get(1)?,
                        artifact: row.get(2)?,
                        revision: u64::from_be_bytes(row.get(3)?),
                        digest: ContentDigest(row.get(4)?),
                    },
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .optional()?;
    let Some((binding, destination, state, remote_id, ipfs)) = record else {
        return Ok(None);
    };
    if binding != intent.artifact || destination != intent.destination {
        return Err(Error::JournalConflict);
    }
    match (state.as_str(), remote_id, ipfs) {
        ("PREPARED", None, None) => Ok(Some(OperationState::Prepared)),
        ("UNRESOLVED", None, None) => Ok(Some(OperationState::Unresolved)),
        ("CONFIRMED", Some(remote_id), ipfs) if valid_identifier(&remote_id) => {
            let ipfs = ipfs
                .map(|value| {
                    VerifiedIpfs::parse(&value, binding.digest).map_err(|_| Error::JournalConflict)
                })
                .transpose()?;
            Ok(Some(OperationState::Confirmed(ConfirmedReceipt {
                remote_id,
                ipfs,
            })))
        }
        _ => Err(Error::JournalConflict),
    }
}
#[cfg(test)]
mod tests;
