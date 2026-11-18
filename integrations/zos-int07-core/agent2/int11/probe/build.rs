// SPDX-License-Identifier: Apache-2.0

use std::{env, fs, path::PathBuf};

fn main() {
    let source = env::var_os("INT11_PASTEBIN_SOURCE").map_or_else(
        || PathBuf::from("../../../../../src/ipfs.rs"),
        PathBuf::from,
    );
    println!("cargo:rerun-if-env-changed=INT11_PASTEBIN_SOURCE");
    println!("cargo:rerun-if-changed={}", source.display());
    let text = fs::read_to_string(source).expect("read audited Pastebin source");
    let start = text.find("fn ipfs_repo()").expect("original writer start");
    let end = text
        .find("/// Quick local content hash")
        .expect("original reader end");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(
        output.join("pastebin_ipfs.rs"),
        &text.as_bytes()[start..end],
    )
    .expect("copy exact writer and reader functions");
}
