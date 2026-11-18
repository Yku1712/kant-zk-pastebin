// SPDX-License-Identifier: Apache-2.0
//! Bounded, borrowing decoder for the DAG-PB and UnixFS protobuf fields used by files.

use crate::{Cid, Error, check_deadline};
use std::time::Instant;

enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

fn varint(input: &mut &[u8]) -> Result<u64, Error> {
    let mut value = 0;
    for offset in (0..=63).step_by(7) {
        let (&byte, remaining) = input.split_first().ok_or(Error::InvalidProtobuf)?;
        *input = remaining;
        if offset == 63 && byte > 1 {
            return Err(Error::InvalidProtobuf);
        }
        value |= u64::from(byte & 0x7f) << offset;
        if byte & 0x80 == 0 {
            if offset != 0 && byte == 0 {
                return Err(Error::InvalidProtobuf);
            }
            return Ok(value);
        }
    }
    Err(Error::InvalidProtobuf)
}

fn field<'a>(input: &mut &'a [u8]) -> Result<(u64, Value<'a>), Error> {
    let key = varint(input)?;
    let tag = key >> 3;
    if !(1..(1 << 29)).contains(&tag) {
        return Err(Error::InvalidProtobuf);
    }
    let value = match key & 7 {
        0 => Value::Varint(varint(input)?),
        2 => {
            let size = usize::try_from(varint(input)?).map_err(|_| Error::InvalidProtobuf)?;
            let (value, rest) = input.split_at_checked(size).ok_or(Error::InvalidProtobuf)?;
            *input = rest;
            Value::Bytes(value)
        }
        _ => return Err(Error::InvalidProtobuf),
    };
    Ok((tag, value))
}

fn once<T>(slot: &mut Option<T>, value: T) -> Result<(), Error> {
    if slot.replace(value).is_some() {
        return Err(Error::InvalidProtobuf);
    }
    Ok(())
}

pub(crate) struct Link {
    pub cid: Cid,
    pub total: Option<u64>,
}

fn link(mut input: &[u8]) -> Result<Link, Error> {
    let (mut cid, mut total) = (None, None);
    let mut previous = 0;
    while !input.is_empty() {
        let (tag, value) = field(&mut input)?;
        if tag <= previous {
            return Err(Error::InvalidProtobuf);
        }
        previous = tag;
        match (tag, value) {
            (1, Value::Bytes(bytes)) => {
                // Largest supported binary CID is CIDv1 with one-byte version/codec
                // and the 34-byte SHA2-256 multihash. Bound parsing before allocation.
                if bytes.len() > 36 {
                    return Err(Error::UnsupportedCid);
                }
                let mut remaining = bytes;
                let parsed = Cid::read_bytes(&mut remaining).map_err(|_| Error::InvalidProtobuf)?;
                if !remaining.is_empty() || parsed.to_bytes() != bytes {
                    return Err(Error::InvalidProtobuf);
                }
                crate::check_cid(&parsed)?;
                cid = Some(parsed);
            }
            (2, Value::Bytes(bytes)) => {
                if !bytes.is_empty() {
                    return Err(Error::InvalidLinks);
                }
            }
            (3, Value::Varint(size)) => total = Some(size),
            _ => return Err(Error::InvalidProtobuf),
        }
    }
    Ok(Link {
        cid: cid.ok_or(Error::InvalidLinks)?,
        total,
    })
}

pub(crate) struct Node<'a> {
    pub data: &'a [u8],
    pub links: Vec<Link>,
}

pub(crate) fn node(
    mut input: &[u8],
    links_cap: usize,
    deadline: Instant,
) -> Result<Node<'_>, Error> {
    let mut data = None;
    let mut links = Vec::new();
    let mut data_after_links = false;
    while !input.is_empty() {
        check_deadline(deadline)?;
        match field(&mut input)? {
            (1, Value::Bytes(bytes)) => {
                once(&mut data, bytes)?;
                data_after_links = !links.is_empty();
            }
            (2, Value::Bytes(bytes)) => {
                if data_after_links {
                    return Err(Error::InvalidProtobuf);
                }
                if links.len() == links_cap {
                    return Err(Error::Limit("nodes"));
                }
                links.push(link(bytes)?);
            }
            _ => return Err(Error::InvalidProtobuf),
        }
    }
    Ok(Node {
        data: data.ok_or(Error::InvalidProtobuf)?,
        links,
    })
}

pub(crate) struct File<'a> {
    pub data: &'a [u8],
    pub size: u64,
    pub child_sizes: Vec<u64>,
}

pub(crate) fn file(mut input: &[u8], links: usize, deadline: Instant) -> Result<File<'_>, Error> {
    let (mut kind, mut data, mut size) = (None, None, None);
    let mut child_sizes = Vec::new();
    let mut add_size = |size| {
        if child_sizes.len() == links {
            return Err(Error::InvalidLinks);
        }
        child_sizes.push(size);
        Ok(())
    };
    while !input.is_empty() {
        check_deadline(deadline)?;
        match field(&mut input)? {
            (1, Value::Varint(kind_value)) => once(&mut kind, kind_value)?,
            (2, Value::Bytes(bytes)) => once(&mut data, bytes)?,
            (3, Value::Varint(bytes)) => once(&mut size, bytes)?,
            (4, Value::Varint(bytes)) => add_size(bytes)?,
            (4, Value::Bytes(mut packed)) => {
                while !packed.is_empty() {
                    check_deadline(deadline)?;
                    add_size(varint(&mut packed)?)?;
                }
            }
            (5..=8, _) => return Err(Error::UnsupportedUnixfs),
            _ => return Err(Error::InvalidProtobuf),
        }
    }
    let data = data.unwrap_or_default();
    let size = match kind {
        Some(0) if links == 0 => size.unwrap_or(0),
        Some(2) => size.unwrap_or(0),
        Some(_) => return Err(Error::UnsupportedUnixfs),
        None => return Err(Error::InvalidProtobuf),
    };
    if child_sizes.len() != links {
        return Err(Error::InvalidLinks);
    }
    let total = child_sizes
        .iter()
        .try_fold(data.len() as u64, |sum, child| {
            sum.checked_add(*child).ok_or(Error::SizeMismatch)
        })?;
    if total != size {
        return Err(Error::SizeMismatch);
    }
    Ok(File {
        data,
        size,
        child_sizes,
    })
}
