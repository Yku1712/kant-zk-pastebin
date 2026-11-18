#!/usr/bin/env python3
"""Generate the compact flatfs fixture catalog using a verified offline Kubo binary."""

import base64
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def kubo(ipfs, repo, *args):
    return subprocess.check_output(
        [ipfs, "--offline", *args], env={**os.environ, "IPFS_PATH": str(repo)}
    )


def main():
    ipfs = str(Path(sys.argv[1]).resolve())
    print(kubo(ipfs, Path(), "version", "--all").decode().strip(), file=sys.stderr)
    cases = [
        ("raw-v1", b"raw CID\x00\xff\n", ["--cid-version=1", "--raw-leaves=true", "--chunker=size-16"]),
        ("file-v0", b"one UnixFS block\x00\n", ["--cid-version=0", "--raw-leaves=false"]),
        ("empty-v0", b"", ["--cid-version=0", "--raw-leaves=false"]),
        ("multiblock-v0", b"first__second_third\x00END\xff", ["--cid-version=0", "--raw-leaves=false", "--chunker=size-7"]),
        ("multiblock-v1", b"zero\x00\xff\nONEtwo\r\nthreeLAST", ["--cid-version=1", "--raw-leaves=true", "--chunker=size-7"]),
        ("nested-v1", b"abcdEFGH" * 175 + b"tail\xff", ["--cid-version=1", "--raw-leaves=true", "--chunker=size-8"]),
    ]
    with tempfile.TemporaryDirectory(prefix="zos-kubo-fixture-") as directory:
        scratch = Path(directory)
        payload_file = scratch / "input"
        for label, content, flags in cases:
            repo = scratch / label
            kubo(ipfs, repo, "init", "--empty-repo")
            payload_file.write_bytes(content)
            root = kubo(ipfs, repo, "add", "-Q", "--pin=false", *flags, str(payload_file)).decode().strip()
            assert kubo(ipfs, repo, "cat", "--progress=false", root) == content
            links = kubo(ipfs, repo, "refs", "--recursive", "--unique", root).decode().splitlines()
            entries = {}
            for cid in [root, *links]:
                block = kubo(ipfs, repo, "block", "get", cid)
                mh = b"\x12\x20" + hashlib.sha256(block).digest()
                key = base64.b32encode(mh).decode().rstrip("=")
                relative = Path("blocks", key[-3:-1], key + ".data")
                assert (repo / relative).read_bytes() == block
                entries[relative] = block
            print(
                f"{label} {root} {hashlib.sha256(content).hexdigest()} {len(entries)} blocks",
                file=sys.stderr,
            )
            print(f"case {label} {root} {content.hex() or '-'}")
            for relative, block in sorted(entries.items()):
                print(f"block {relative.as_posix()} {block.hex() or '-'}")
    print("Every payload independently matched Kubo's offline cat.", file=sys.stderr)


if __name__ == "__main__":
    main()
