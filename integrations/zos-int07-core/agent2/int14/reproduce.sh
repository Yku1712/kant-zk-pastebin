#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source_repo=$(git -C "$here" rev-parse --show-toplevel)
pin=113ec894865a6f16c8d8f9bdfc559c72abd02c82
base=361fae9395e4c7a18bbecf0099f2c613cc785d6d
writer=8cb627b41bd53a555557d7e46a5c1c1070d15dde
legacy=a9dde4d712dc0c99c3ef2b30d92ef49723695c77
area=$(mktemp -d "${1:-${TMPDIR:-/tmp}}/zos-int14.XXXXXXXX")
printf 'Evidence workspace %s\n' "$area"
exec > >(tee "$area/transcript.log") 2>&1
trap 'code=$?; printf "reproduce.sh exit=%s\n" "$code"' EXIT
set -x
rustc -vV
cargo -vV
cc --version
git --version
uname -sm
stat -f -c '%T' "$area"

# Use the checkout as a Git object source. Fetch missing objects from the public remote.
git -C "$source_repo" cat-file -e "$pin^{commit}" ||
    git -C "$source_repo" fetch --no-tags origin "$pin"
git -C "$source_repo" cat-file -e "$legacy^{commit}" ||
    git -C "$source_repo" fetch --no-tags origin "$legacy"
test "$(git -C "$source_repo" rev-parse "$pin^")" = "$base"
git -C "$source_repo" rev-parse "$pin" "$pin^{tree}" "$writer" "$legacy"
mkdir "$area/export" "$area/writer" "$area/legacy" "$area/compat"
git -C "$source_repo" archive "$pin" | tar -x -C "$area/export"
git -C "$source_repo" archive "$writer" Cargo.toml src/ipfs.rs | tar -x -C "$area/writer"
suite=integrations/zos-int08-authorized-artifact-bridge
audit="$area/export/integrations/zos-int07-core/agent2"
git -C "$area/writer" apply "$audit/int11/INT11_FAIL_CLOSED_PRODUCER.patch"
sha256sum "$area/writer/src/ipfs.rs" "$audit/int11/INT11_FAIL_CLOSED_PRODUCER.patch"
(cd "$audit/int13" && sha256sum -c SHA256SUMS)
INT11_PASTEBIN_SOURCE="$area/writer/src/ipfs.rs" cargo build --locked \
    --manifest-path "$audit/int11/probe/Cargo.toml" --features patched-writer \
    --target-dir "$area/producer-target"
export INT13_PROBE="$area/producer-target/debug/pastebin-writer-probe"
sha256sum "$INT13_PROBE"
export CARGO_TARGET_DIR="$area/int13-target"
cargo test --locked --manifest-path "$audit/int13/Cargo.toml" \
    -- --ignored --skip tests::crash_child --nocapture
cargo test --locked --manifest-path "$audit/int13/Cargo.toml" \
    -- --ignored --exact tests::sigkill_before_and_after_destination_commit_reconciles_without_duplicate_receipts --nocapture

cp "$here/int14_tests.rs" "$audit/int13/src/int14.rs"
printf '\n#[path = "int14.rs"]\nmod int14;\n' >> "$audit/int13/src/tests.rs"
cargo test --locked --manifest-path "$audit/int13/Cargo.toml" -- --ignored tests::int14:: --nocapture

git -C "$source_repo" archive "$legacy" "$suite" | tar -x -C "$area/legacy"
(cd "$area/legacy/$suite" && sha256sum -c "$here/INT08_GIT_SHA256SUMS")
python3 "$here/verify_legacy_manifest.py" "$source_repo" "$area/legacy" "$here/INT08_GIT_SHA256SUMS"
git -C "$source_repo" archive "$pin" integrations/zos-int07-core "$suite" | tar -x -C "$area/compat"
cp -r "$area/legacy/$suite/tests" "$area/compat/$suite/"
cp "$here/INT08_COMPAT.Cargo.lock" "$area/compat/$suite/Cargo.lock"
set +e
cargo test --locked --manifest-path "$area/compat/$suite/Cargo.toml" 2>&1 | tee "$area/e0050.log"
code=${PIPESTATUS[0]}
set -e
printf 'unpatched legacy tests exit=%s\n' "$code"
test "$code" = 101
test "$(rg -c '^error\[E0050\]' "$area/e0050.log")" = 2
git -C "$area/compat" apply --check "$here/INT08_TEST_FIXTURES.patch"
git -C "$area/compat" apply "$here/INT08_TEST_FIXTURES.patch"
cargo test --locked --manifest-path "$area/compat/$suite/Cargo.toml"
