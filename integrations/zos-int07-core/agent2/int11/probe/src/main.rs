// SPDX-License-Identifier: Apache-2.0

use std::{
    env, fs,
    io::{self, Write},
};

#[allow(dead_code)]
mod pastebin {
    use ipld_core::cid::Cid;
    use rust_unixfs::file::adder::FileAdder;

    include!(concat!(env!("OUT_DIR"), "/pastebin_ipfs.rs"));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [mode, path] if mode == "add" => {
            let bytes = fs::read(path)?;
            println!(
                "{}",
                pastebin::ipfs_add_bytes(&bytes)
                    .as_deref()
                    .unwrap_or("NONE")
            );
        }
        [mode, cid] if mode == "cat-block" => {
            let block = pastebin::ipfs_cat(cid.to_str().ok_or("non-UTF-8 CID")?)
                .ok_or("local block unavailable")?;
            io::stdout().lock().write_all(&block)?;
        }
        _ => return Err("usage: pastebin-writer-probe {add PATH|cat-block CID}".into()),
    }
    Ok(())
}
