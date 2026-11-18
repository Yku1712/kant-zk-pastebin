#!/usr/bin/env python3
"""Compare isolated probe archives against the reviewed Pastebin lockfile."""

from hashlib import sha256
from pathlib import Path
import subprocess
import sys
import tomllib


EXPECTED_PUBLIC_ARCHIVES = {
    "rust-unixfs", "ipld-dagpb", "quick-protobuf", "ipld-core", "cid",
    "multihash", "multihash-codetable", "multihash-derive", "multihash-derive-impl",
    "sha2", "bytes", "either", "filetime", "data-encoding", "log",
}


def packages(path):
    with open(path, "rb") as source:
        return tomllib.load(source)["package"]


def main():
    original_path, cache = map(Path, sys.argv[1:])
    probe_manifest = Path(__file__).with_name("Cargo.toml")
    active = subprocess.check_output(
        ["cargo", "tree", "--locked", "--offline", "--prefix", "none", "--format",
         "{p}", "--edges", "normal,build", "--manifest-path", str(probe_manifest)],
        text=True,
    )
    active_packages = {(name, version.removeprefix("v")) for name, version, *_ in
                       (row.split() for row in active.splitlines())}

    original = {(p["name"], p["version"], p.get("checksum")) for p in packages(original_path)}
    remaining = EXPECTED_PUBLIC_ARCHIVES.copy()
    differences = []
    for package in packages(probe_manifest.with_name("Cargo.lock")):
        name, version = package["name"], package["version"]
        expected = package.get("checksum")
        if (name, version) not in active_packages or expected is None:
            continue
        assert package["source"] == "registry+https://github.com/rust-lang/crates.io-index"
        identity = (name, version, expected)
        if identity not in original:
            differences.append(f"{name} {version}")
        if name not in EXPECTED_PUBLIC_ARCHIVES:
            continue
        assert identity in original, identity
        archives = sorted(cache.glob(f"*/{name}-{version}.crate"))
        assert archives, (name, version)
        actual = sha256(archives[0].read_bytes()).hexdigest()
        assert actual == expected, (name, version, actual)
        print(f"{actual}  {name}-{version}.crate  LOCK AND ARCHIVE MATCH")
        remaining.discard(name)
    assert not remaining, remaining
    print("Other active probe packages with differing versions or checksums:", ", ".join(differences))


if __name__ == "__main__":
    main()
