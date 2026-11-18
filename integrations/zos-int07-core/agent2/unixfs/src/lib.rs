// SPDX-License-Identifier: Apache-2.0
//! Bounded local verification for raw CID blocks and DAG-PB UnixFS files.
#![forbid(unsafe_code)]

#[cfg(unix)]
mod flatfs;
#[cfg(feature = "int07")]
mod int07;
mod wire;

pub use cid::Cid;
#[cfg(unix)]
pub use flatfs::Flatfs;
#[cfg(feature = "int07")]
pub use int07::UnixfsReceiptEvidence;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fmt, io,
    time::{Duration, Instant},
};

const RAW: u64 = 0x55;
const DAG_PB: u64 = 0x70;
const SHA2_256: u64 = 0x12;

#[derive(Debug)]
pub enum Error {
    MissingBlock(Cid),
    HashMismatch(Cid),
    UnsupportedCid,
    UnsupportedUnixfs,
    InvalidProtobuf,
    InvalidLinks,
    SizeMismatch,
    Cycle,
    Limit(&'static str),
    Deadline,
    DigestMismatch,
    Io(io::Error),
    Output(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingBlock(cid) => write!(f, "missing block {cid}"),
            Self::HashMismatch(cid) => write!(f, "block digest mismatch for {cid}"),
            Self::UnsupportedCid => f.write_str("unsupported CID codec or multihash"),
            Self::UnsupportedUnixfs => f.write_str("unsupported UnixFS type or metadata"),
            Self::InvalidProtobuf => f.write_str("invalid DAG-PB or UnixFS protobuf"),
            Self::InvalidLinks => f.write_str("invalid UnixFS file links"),
            Self::SizeMismatch => f.write_str("UnixFS declared size mismatch"),
            Self::Cycle => f.write_str("cycle in UnixFS links"),
            Self::Limit(name) => write!(f, "{name} limit exceeded"),
            Self::Deadline => f.write_str("verification deadline elapsed"),
            Self::DigestMismatch => {
                f.write_str("reconstructed bytes do not match authoritative SHA-256")
            }
            Self::Io(error) => write!(f, "block I/O: {error}"),
            Self::Output(error) => write!(f, "verification output: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) | Self::Output(error) => Some(error),
            _ => None,
        }
    }
}

/// A bounded block retrieval port. Implementations must cap I/O and allocation at
/// `limit`, detect oversized blocks, and honor the cooperative deadline.
pub trait BlockSource {
    fn read_block(&self, cid: &Cid, limit: usize, deadline: Instant) -> Result<Vec<u8>, Error>;
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub block_bytes: usize,
    pub encoded_bytes: usize,
    pub output_bytes: usize,
    pub nodes: usize,
    /// Root depth is zero.
    pub depth: usize,
    pub time: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            block_bytes: 1024 * 1024,
            encoded_bytes: 8 * 1024 * 1024,
            output_bytes: 1024 * 1024,
            nodes: 4096,
            depth: 32,
            time: Duration::from_secs(30),
        }
    }
}

impl Limits {
    fn validate(self) -> Result<(), Error> {
        let cap = Self::default();
        for (value, max, label) in [
            (self.block_bytes, cap.block_bytes, "block bytes"),
            (self.encoded_bytes, cap.encoded_bytes, "encoded bytes"),
            (self.output_bytes, cap.output_bytes, "output bytes"),
            (self.nodes, cap.nodes, "nodes"),
            (self.depth, cap.depth, "depth"),
        ] {
            if value > max {
                return Err(Error::Limit(label));
            }
        }
        if self.time > cap.time {
            return Err(Error::Limit("time"));
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Verified {
    pub bytes: usize,
    pub encoded_bytes: usize,
    pub nodes: usize,
    pub sha256: [u8; 32],
}

/// Reconstruct a file in authentic link order. Each block is hash-checked before
/// parsing or emitting its content. The receiver must stage or discard streamed
/// output until this function returns success, since a late child or digest can fail.
pub fn verify(
    source: &impl BlockSource,
    root: &Cid,
    expected_sha256: [u8; 32],
    limits: Limits,
    output: impl FnMut(&[u8]) -> io::Result<()>,
) -> Result<Verified, Error> {
    limits.validate()?;
    let deadline = Instant::now() + limits.time;
    let mut walk = Walk {
        source,
        limits,
        deadline,
        output,
        hash: Sha256::new(),
        active: HashSet::new(),
        bytes: 0,
        encoded: 0,
        nodes: 0,
    };
    walk.visit(root, 0)?;
    check_deadline(deadline)?;
    let sha256 = walk.hash.finalize().into();
    if sha256 != expected_sha256 {
        return Err(Error::DigestMismatch);
    }
    Ok(Verified {
        bytes: walk.bytes,
        encoded_bytes: walk.encoded,
        nodes: walk.nodes,
        sha256,
    })
}

pub(crate) fn check_deadline(deadline: Instant) -> Result<(), Error> {
    if Instant::now() >= deadline {
        return Err(Error::Deadline);
    }
    Ok(())
}

fn check_cid(cid: &Cid) -> Result<(), Error> {
    if !matches!(cid.codec(), RAW | DAG_PB)
        || cid.hash().code() != SHA2_256
        || cid.hash().size() != 32
    {
        return Err(Error::UnsupportedCid);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Size {
    file: u64,
    dag: u64,
}

struct Walk<'a, S, O> {
    source: &'a S,
    limits: Limits,
    deadline: Instant,
    output: O,
    hash: Sha256,
    active: HashSet<Cid>,
    bytes: usize,
    encoded: usize,
    nodes: usize,
}

impl<S: BlockSource, O: FnMut(&[u8]) -> io::Result<()>> Walk<'_, S, O> {
    fn visit(&mut self, cid: &Cid, depth: usize) -> Result<Size, Error> {
        check_deadline(self.deadline)?;
        if depth > self.limits.depth {
            return Err(Error::Limit("depth"));
        }
        if self.nodes == self.limits.nodes {
            return Err(Error::Limit("nodes"));
        }
        check_cid(cid)?;
        if !self.active.insert(*cid) {
            return Err(Error::Cycle);
        }
        self.nodes += 1;
        let result = self.read_verified(cid, depth);
        self.active.remove(cid);
        result
    }

    fn read_verified(&mut self, cid: &Cid, depth: usize) -> Result<Size, Error> {
        let limit = self
            .limits
            .block_bytes
            .min(self.limits.encoded_bytes - self.encoded);
        let block = self.source.read_block(cid, limit, self.deadline)?;
        check_deadline(self.deadline)?;
        if block.len() > limit {
            return Err(Error::Limit("block or encoded bytes"));
        }
        self.encoded += block.len();
        if Sha256::digest(&block).as_slice() != cid.hash().digest() {
            return Err(Error::HashMismatch(*cid));
        }
        if cid.codec() == RAW {
            self.emit(&block)?;
            return Ok(Size {
                file: block.len() as u64,
                dag: block.len() as u64,
            });
        }

        let available_visits = self.limits.nodes - self.nodes;
        let node = wire::node(&block, available_visits, self.deadline)?;
        let file = wire::file(node.data, node.links.len(), self.deadline)?;
        if file.size > (self.limits.output_bytes - self.bytes) as u64 {
            return Err(Error::Limit("output bytes"));
        }
        self.emit(file.data)?;
        let mut dag = block.len() as u64;
        for (link, expected_file) in node.links.iter().zip(file.child_sizes) {
            let child = self.visit(&link.cid, depth + 1)?;
            if child.file != expected_file
                || link.total.is_some_and(|expected| expected != child.dag)
            {
                return Err(Error::SizeMismatch);
            }
            dag = dag.checked_add(child.dag).ok_or(Error::SizeMismatch)?;
        }
        Ok(Size {
            file: file.size,
            dag,
        })
    }

    fn emit(&mut self, bytes: &[u8]) -> Result<(), Error> {
        check_deadline(self.deadline)?;
        if bytes.len() > self.limits.output_bytes - self.bytes {
            return Err(Error::Limit("output bytes"));
        }
        (self.output)(bytes).map_err(Error::Output)?;
        self.hash.update(bytes);
        self.bytes += bytes.len();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cid::multihash::Multihash;

    struct NeverRead;
    impl BlockSource for NeverRead {
        fn read_block(&self, _: &Cid, _: usize, _: Instant) -> Result<Vec<u8>, Error> {
            panic!("active ancestor must be rejected before I/O")
        }
    }

    #[test]
    fn active_ancestor_is_rejected_before_io() {
        let cid = Cid::new_v1(RAW, Multihash::wrap(SHA2_256, &[0; 32]).unwrap());
        let limits = Limits::default();
        let mut walk = Walk {
            source: &NeverRead,
            limits,
            deadline: Instant::now() + limits.time,
            output: |_: &[u8]| Ok(()),
            hash: Sha256::new(),
            active: HashSet::from([cid]),
            bytes: 0,
            encoded: 0,
            nodes: 0,
        };
        // Content-addressed fixtures cannot embed their own valid SHA-256 CID.
        assert!(matches!(walk.visit(&cid, 1), Err(Error::Cycle)));
    }
}
