# SPDX-License-Identifier: Apache-2.0
"""Verify the INT08 historical and exact manifests against the pinned Git objects."""
import hashlib
from pathlib import Path
import subprocess
import sys

PIN = "a9dde4d712dc0c99c3ef2b30d92ef49723695c77"
DIRECTORY = "integrations/zos-int08-authorized-artifact-bridge"


def main(repo, exported_tree, exact_manifest):
    def read_blob(path):
        return subprocess.check_output(
            ["git", "-C", repo, "cat-file", "blob", f"{PIN}:{DIRECTORY}/{path}"]
        )

    def digest(data):
        return hashlib.sha256(data).hexdigest()

    historical = read_blob("SHA256SUMS").decode().splitlines()
    assert len(historical) == 13
    exact = []
    for entry in historical:
        claimed, path = entry.split("  ", 1)
        data = read_blob(path)
        assert (Path(exported_tree) / DIRECTORY / path).read_bytes() == data
        blob = digest(data)
        crlf = digest(data.replace(b"\n", b"\r\n"))
        assert claimed != blob and claimed == crlf and b"\r\n" not in data
        exact.append(f"{blob}  {path}\n")
        print(f"{path} Git/archive={blob} historical={claimed} CRLF={crlf}")
    assert Path(exact_manifest).read_bytes() == "".join(exact).encode()
    print(f"{PIN}: 13 verified Git hashes and 13 historical CRLF matches")


if __name__ == "__main__":
    main(*sys.argv[1:])
