/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Data directories need to be mangled a little bit for Buck2/RE
//! * cannot have any slashes
//! * cannot have any unreadable mode bits
//! * cannot have device nodes
//!
//! This module contains functions to mangle and unmangle these directories.

use std::ffi::OsStr;
use std::fs::create_dir_all;
use std::fs::File;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::FileTypeExt as _;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use urlencoding::decode_binary;
use urlencoding::encode_binary;
use walkdir::WalkDir;

pub fn mangle(src: &Path, dst_root: &Path) -> Result<()> {
    create_dir_all(dst_root)?;
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let relpath = entry
            .path()
            .strip_prefix(src)
            .context("path not under root")?;
        let dst = dst_root.join(encode_binary(relpath.as_os_str().as_bytes()).as_ref());
        let ft = entry.file_type();
        // only files need to be copied
        if ft.is_file()
            && !(ft.is_symlink() || ft.is_char_device() || ft.is_block_device() || ft.is_fifo())
        {
            let mut f = File::create(&dst)
                .with_context(|| format!("while creating dst file '{}'", dst.display()))?;
            let mut src_f = File::open(entry.path())?;
            std::io::copy(&mut src_f, &mut f)?;
        }
    }
    Ok(())
}

#[tracing::instrument(ret, err)]
pub fn unmangle(src: &Path, dst_root: &Path) -> Result<()> {
    for entry in
        std::fs::read_dir(src).with_context(|| format!("while reading dir '{}'", src.display()))?
    {
        let entry = entry?;
        let urlencoded_path = entry.file_name();
        let decoded = decode_binary(urlencoded_path.as_bytes());
        let dst = dst_root.join(OsStr::from_bytes(&decoded));
        if let Some(parent) = dst.parent() {
            create_dir_all(parent)?;
        }
        let mut f = File::create(&dst)?;
        let mut src_f = File::open(entry.path())?;
        std::io::copy(&mut src_f, &mut f)?;
    }
    Ok(())
}
