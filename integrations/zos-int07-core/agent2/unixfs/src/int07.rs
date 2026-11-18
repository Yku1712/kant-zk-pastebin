// SPDX-License-Identifier: Apache-2.0

use crate::{BlockSource, Cid, Error, Limits, verify};
use std::io;
use zos_int07_core::{
    Error as CoreError, EvidenceOutput, Intent, ReceiptCandidate, ReceiptEvidence,
};

/// Wrap existing receipt provenance and original-byte evidence with verified UnixFS readback.
/// The wrapped provider still authenticates operation, receipt, and claimed root correlation.
pub struct UnixfsReceiptEvidence<E, S> {
    pub original: E,
    pub blocks: S,
    pub limits: Limits,
}

impl<E: ReceiptEvidence, S: BlockSource> ReceiptEvidence for UnixfsReceiptEvidence<E, S> {
    fn published_bytes(
        &self,
        intent: &Intent,
        receipt: &ReceiptCandidate,
        output: &mut dyn EvidenceOutput,
    ) -> Result<(), CoreError> {
        if receipt.ipfs_cid.is_none() {
            return Err(CoreError::InvalidReceipt);
        }
        self.original.published_bytes(intent, receipt, output)
    }

    fn verified_ipfs_bytes(
        &self,
        intent: &Intent,
        root: &Cid,
        output: &mut dyn EvidenceOutput,
    ) -> Result<(), CoreError> {
        let mut limits = self.limits;
        limits.output_bytes = limits.output_bytes.min(output.remaining());
        verify(
            &self.blocks,
            root,
            intent.artifact().digest.0,
            limits,
            |chunk| output.append(chunk).map_err(io::Error::other),
        )
        .map(|_| ())
        .map_err(|error| match error {
            Error::MissingBlock(_) | Error::Deadline | Error::Io(_) => {
                CoreError::ExternalUnavailable
            }
            Error::Limit(_) | Error::Output(_) => CoreError::ResourceLimit,
            Error::DigestMismatch => CoreError::DigestMismatch,
            _ => CoreError::InvalidReceipt,
        })
    }
}
