// SPDX-License-Identifier: Apache-2.0
#![cfg(unix)]

use data_encoding::BASE32_NOPAD;
use sha2::{Digest, Sha256};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
use zos_unixfs_readback::{Cid, Error, Flatfs, Limits, verify};

const CHUNK: usize = 256 * 1024;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = env::temp_dir().join(format!(
                "zos-int11-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("fixture directory: {error}"),
            }
        }
    }

    fn directory(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir(&path).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Probe {
    binary: PathBuf,
    patched: bool,
}

fn successful(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "child exited {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

impl Probe {
    fn from_env() -> Self {
        let binary = env::var_os("INT11_PROBE").expect("path to compiled Pastebin writer probe");
        let patched = match env::var("INT11_MODE").as_deref() {
            Ok("pinned") => false,
            Ok("patched") => true,
            _ => panic!("INT11_MODE must be pinned or patched"),
        };
        Self {
            binary: binary.into(),
            patched,
        }
    }

    fn add(&self, repo: &Path, input: &Path) -> Option<Cid> {
        let out = successful(
            Command::new(&self.binary)
                .env("IPFS_PATH", repo)
                .arg("add")
                .arg(input)
                .output()
                .unwrap(),
        );
        let result = std::str::from_utf8(&out).unwrap().trim();
        (result != "NONE").then(|| {
            let cid: Cid = result.parse().unwrap();
            assert_eq!(cid.codec(), 0x70);
            assert_eq!(cid.to_string(), result);
            assert!(result.starts_with("Qm"));
            cid
        })
    }

    fn cat_block(&self, repo: &Path, cid: &Cid) -> Vec<u8> {
        successful(
            Command::new(&self.binary)
                .env("IPFS_PATH", repo)
                .args(["cat-block", &cid.to_string()])
                .output()
                .unwrap(),
        )
    }
}

fn digest(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

fn payload(size: usize) -> Vec<u8> {
    (0..size).map(|i| ((i / CHUNK) ^ (i % 251)) as u8).collect()
}

fn write_input(temp: &Scratch, data: &[u8]) -> PathBuf {
    let input = temp.0.join("input");
    fs::write(&input, data).unwrap();
    input
}

fn block_path(repo: &Path, cid: &Cid) -> PathBuf {
    let key = BASE32_NOPAD.encode(&cid.hash().to_bytes());
    repo.join("blocks")
        .join(&key[key.len() - 3..key.len() - 1])
        .join(format!("{key}.data"))
}

fn reconstruct(repo: &Path, cid: &Cid, expected: &[u8]) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    verify(
        &Flatfs::open(repo)?,
        cid,
        digest(expected),
        Limits::default(),
        |part| {
            bytes.extend_from_slice(part);
            Ok(())
        },
    )?;
    Ok(bytes)
}

#[test]
#[ignore = "requires the separately locked INT11 original or patched writer probe"]
fn authentic_producer_empty_and_chunk_boundaries() {
    let probe = Probe::from_env();
    for len in [
        0,
        31,
        CHUNK - 1,
        CHUNK,
        CHUNK + 1,
        2 * CHUNK - 1,
        2 * CHUNK,
        2 * CHUNK + 1,
        3 * CHUNK + 17,
        4 * CHUNK,
    ] {
        let temp = Scratch::new();
        let bytes = payload(len);
        let input = write_input(&temp, &bytes);
        let repo = temp.directory("repo");
        let root = probe
            .add(&repo, &input)
            .expect("valid input must return a root");
        assert!(block_path(&repo, &root).is_file());
        let direct = probe.cat_block(&repo, &root);
        assert_eq!(digest(&direct).as_slice(), root.hash().digest());
        assert_eq!(direct, fs::read(block_path(&repo, &root)).unwrap());
        assert_ne!(direct, bytes, "the Pastebin reader returns a DAG-PB block");

        if len <= 2 * CHUNK || probe.patched {
            assert_eq!(reconstruct(&repo, &root, &bytes).unwrap(), bytes);
            println!("size={len} root={root} outcome=complete");
        } else {
            assert!(matches!(
                reconstruct(&repo, &root, &bytes),
                Err(Error::DigestMismatch)
            ));
            let prefix = &bytes[..2 * CHUNK];
            assert_eq!(reconstruct(&repo, &root, prefix).unwrap(), prefix);
            println!(
                "size={len} root={root} outcome=retained-prefix length={}",
                prefix.len()
            );
        }
    }
}

#[test]
#[ignore = "requires the separately locked INT11 original or patched writer probe"]
fn authentic_producer_storage_and_partial_persistence() {
    let probe = Probe::from_env();
    let temp = Scratch::new();
    let bytes = payload(CHUNK + 29);
    let input = write_input(&temp, &bytes);

    let missing = temp.0.join("missing-repo");
    assert_eq!(probe.add(&missing, &input).is_none(), probe.patched);
    assert!(!missing.exists());

    let blocked = temp.directory("blocked-repo");
    fs::write(blocked.join("blocks"), b"not a directory").unwrap();
    let claimed = probe.add(&blocked, &input);
    assert_eq!(claimed.is_none(), probe.patched);
    if let Some(root) = claimed {
        assert!(matches!(
            reconstruct(&blocked, &root, &bytes),
            Err(Error::Io(_))
        ));
    }

    let first_repo = temp.directory("first-leaf");
    let first_input = temp.0.join("first");
    fs::write(&first_input, &bytes[..CHUNK]).unwrap();
    let first = probe.add(&first_repo, &first_input).unwrap();
    let last_repo = temp.directory("last-leaf");
    let last_input = temp.0.join("last");
    fs::write(&last_input, &bytes[CHUNK..]).unwrap();
    let last = probe.add(&last_repo, &last_input).unwrap();
    let partial = temp.directory("partial-repo");
    let obstructed = block_path(&partial, &last);
    fs::create_dir_all(&obstructed).unwrap();

    let root = probe.add(&partial, &input);
    assert!(block_path(&partial, &first).is_file());
    assert!(obstructed.is_dir());
    assert_eq!(root.is_none(), probe.patched);
    if let Some(root) = root {
        assert!(block_path(&partial, &root).is_file());
        assert!(matches!(
            reconstruct(&partial, &root, &bytes),
            Err(Error::Io(_))
        ));
    }
    println!(
        "missing and obstructed repos; earlier leaf retained in partial repo; root withheld={}",
        probe.patched
    );
}

#[test]
#[ignore = "requires the separately locked INT11 original or patched writer probe"]
fn authentic_output_is_rejected_after_child_loss_or_corruption() {
    let probe = Probe::from_env();
    let temp = Scratch::new();
    let bytes = payload(CHUNK + 1);
    let input = write_input(&temp, &bytes);
    let repo = temp.directory("repo");
    let root = probe.add(&repo, &input).unwrap();

    let child_repo = temp.directory("single-child-repo");
    let child_input = temp.0.join("child");
    fs::write(&child_input, &bytes[..CHUNK]).unwrap();
    let child = probe.add(&child_repo, &child_input).unwrap();
    let path = block_path(&repo, &child);
    let original = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(
        matches!(reconstruct(&repo, &root, &bytes), Err(Error::MissingBlock(cid)) if cid == child)
    );
    fs::write(&path, &original[..original.len() - 1]).unwrap();
    assert!(
        matches!(reconstruct(&repo, &root, &bytes), Err(Error::HashMismatch(cid)) if cid == child)
    );
    fs::write(&path, original).unwrap();
    assert_eq!(reconstruct(&repo, &root, &bytes).unwrap(), bytes);
}

#[test]
#[ignore = "requires the separately locked INT11 original or patched writer probe"]
fn authentic_default_nested_input_hits_the_int10_limit_only_after_writer_fix() {
    let probe = Probe::from_env();
    let temp = Scratch::new();
    let bytes = payload(174 * CHUNK + 1);
    let input = write_input(&temp, &bytes);
    let repo = temp.directory("repo");

    let kubo = env::var_os("INT11_KUBO");
    if let Some(kubo) = &kubo {
        successful(
            Command::new(kubo)
                .env("IPFS_PATH", &repo)
                .args(["--offline", "init", "--empty-repo"])
                .output()
                .unwrap(),
        );
    }
    let root = probe.add(&repo, &input).unwrap();
    let result = reconstruct(&repo, &root, &bytes);
    let expected = if probe.patched {
        assert!(matches!(result, Err(Error::Limit("output bytes"))));
        bytes.as_slice()
    } else {
        assert!(matches!(result, Err(Error::DigestMismatch)));
        &bytes[..2 * CHUNK]
    };

    if let Some(kubo) = &kubo {
        let reconstructed = successful(
            Command::new(kubo)
                .env("IPFS_PATH", &repo)
                .args(["--offline", "cat", "--progress=false", &root.to_string()])
                .output()
                .unwrap(),
        );
        assert_eq!(reconstructed, expected);
        let root_text = root.to_string();
        let refs = successful(
            Command::new(kubo)
                .env("IPFS_PATH", &repo)
                .args(["--offline", "refs", "--recursive", "--edges", &root_text])
                .output()
                .unwrap(),
        );
        let edges = std::str::from_utf8(&refs).unwrap();
        let has_deeper_link = edges
            .lines()
            .map(|line| line.split_once(" -> ").expect("Kubo edge format").0)
            .any(|parent| parent != root_text);
        assert_eq!(has_deeper_link, probe.patched);
        println!("offline graph contains depth-two links: {has_deeper_link}");
        println!(
            "Kubo offline cat matched {} bytes from authentic default writer",
            expected.len()
        );
    }
    println!(
        "default nested input={} root={root} recorded-bytes={}",
        bytes.len(),
        expected.len()
    );
}
