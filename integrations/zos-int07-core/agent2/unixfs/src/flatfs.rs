// SPDX-License-Identifier: Apache-2.0

use crate::{BlockSource, Cid, Error, Limits, check_cid, check_deadline};
use data_encoding::BASE32_NOPAD;
use rustix::fs::{Mode, OFlags, open, openat};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    time::Instant,
};

/// Pastebin's next-to-last/2 flatfs layout, rooted at an explicit local repository.
/// The repository path and filesystem are deployment trust boundaries.
pub struct Flatfs {
    blocks: File,
}

const DIR: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

impl Flatfs {
    pub fn open(repo: impl AsRef<Path>) -> Result<Self, Error> {
        let blocks = open(repo.as_ref().join("blocks"), DIR, Mode::empty())
            .map_err(|e| Error::Io(e.into()))?;
        Ok(Self {
            blocks: File::from(blocks),
        })
    }
}

fn open_error(error: rustix::io::Errno, cid: &Cid) -> Error {
    let error: io::Error = error.into();
    if error.kind() == io::ErrorKind::NotFound {
        Error::MissingBlock(*cid)
    } else {
        Error::Io(error)
    }
}

impl BlockSource for Flatfs {
    fn read_block(&self, cid: &Cid, limit: usize, deadline: Instant) -> Result<Vec<u8>, Error> {
        check_deadline(deadline)?;
        check_cid(cid)?;
        let limit = limit.min(Limits::default().block_bytes);
        let key = BASE32_NOPAD.encode(&cid.hash().to_bytes());
        let shard = &key[key.len() - 3..key.len() - 1];
        let directory =
            openat(&self.blocks, shard, DIR, Mode::empty()).map_err(|e| open_error(e, cid))?;
        let desc = openat(
            directory,
            format!("{key}.data"),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| open_error(e, cid))?;
        let mut file = File::from(desc);
        let metadata = file.metadata().map_err(Error::Io)?;
        if !metadata.is_file() {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "flatfs entry is not a regular file",
            )));
        }
        if metadata.len() > limit as u64 {
            return Err(Error::Limit("block or encoded bytes"));
        }
        let mut result = Vec::new();
        result
            .try_reserve_exact(metadata.len() as usize)
            .map_err(|_| Error::Limit("block allocation"))?;
        let mut chunk = [0; 8192];
        loop {
            check_deadline(deadline)?;
            let allowed = (limit - result.len()).saturating_add(1).min(chunk.len());
            let count = match file.read(&mut chunk[..allowed]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                other => other.map_err(Error::Io)?,
            };
            if count == 0 {
                check_deadline(deadline)?;
                return Ok(result);
            }
            if count > limit - result.len() {
                return Err(Error::Limit("block or encoded bytes"));
            }
            result
                .try_reserve_exact(count)
                .map_err(|_| Error::Limit("block allocation"))?;
            result.extend_from_slice(&chunk[..count]);
        }
    }
}
