/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Simple binary to render metadata from a built image used for quick tests of
//! build output. Similar to, but not as pretty as `tree`.
//!
//! Why not just use `tree`?
//! `tree` has options to print the owner of a file, but it always uses the host
//! so is likely to be wrong when called on an image with its own distinct set
//! of uids/gids.
//!
//! This will eventually be (partially) superseded by
//! https://github.com/vmagro/filesystem_in_a_file/ for in-depth filesystem test
//! assertions, but it serves as a quick test during antlir2 development before
//! that is fully ready.

#![feature(io_error_other)]

use std::io::Result;
use std::io::Write;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use antlir2_users::GroupId;
use antlir2_users::Id;
use antlir2_users::UserId;
use clap::Parser;
use walkdir::WalkDir;

#[derive(Parser)]
struct Args {
    root: PathBuf,
    #[clap(long, default_value = "-")]
    out: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let passwd_path = args.root.join("etc/passwd");
    let passwd = if passwd_path.exists() {
        antlir2_users::passwd::EtcPasswd::parse(&std::fs::read_to_string(passwd_path)?)
            .map_err(std::io::Error::other)?
            .into_owned()
    } else {
        Default::default()
    };
    let group_path = args.root.join("etc/group");
    let groups = if group_path.exists() {
        antlir2_users::group::EtcGroup::parse(&std::fs::read_to_string(group_path)?)
            .map_err(std::io::Error::other)?
            .into_owned()
    } else {
        Default::default()
    };

    let mut out = stdio_path::create(&args.out)?;

    for entry in WalkDir::new(&args.root).sort_by(|a, b| a.file_name().cmp(b.file_name())) {
        let entry = entry?;
        let meta = entry.metadata()?;
        let depth = entry.depth();

        let path = entry
            .path()
            .strip_prefix(&args.root)
            .expect("must be there");
        let path: PathBuf = path.iter().skip(depth.saturating_sub(1)).collect();

        if entry.file_type().is_dir() {}
        write!(out, "{}", "│  ".repeat(depth))?;
        write!(out, "├─ ")?;
        write!(out, "{} ", path.display())?;
        if depth != 0 && meta.file_type().is_symlink() {
            let target = std::fs::read_link(entry.path())?;
            write!(out, "-> {} ", target.display())?;
        }
        writeln!(
            out,
            "[{} {}:{}]",
            symbolic_mode(&meta),
            passwd
                .get_user_by_id(UserId::from_raw(meta.uid()))
                .expect("users must exist in the image")
                .name,
            groups
                .get_group_by_id(GroupId::from_raw(meta.gid()))
                .expect("groups must exist in the image")
                .name,
        )?;
    }

    Ok(())
}

/// Generate some kind of symbolic mode representation.
fn symbolic_mode(m: &std::fs::Metadata) -> String {
    let mut s = String::new();
    s.push(symbolic_file_type(m.file_type()));
    s.push_str(&symbolic_permission(m.mode() >> 6)); // u
    s.push_str(&symbolic_permission(m.mode() >> 3)); // g
    s.push_str(&symbolic_permission(m.mode())); // o
    s
}

fn symbolic_file_type(ft: std::fs::FileType) -> char {
    if ft.is_block_device() {
        'b'
    } else if ft.is_char_device() {
        'c'
    } else if ft.is_socket() {
        's'
    } else if ft.is_fifo() {
        'f'
    } else if ft.is_dir() {
        'd'
    } else if ft.is_symlink() {
        'l'
    } else if ft.is_file() {
        '-'
    } else {
        unreachable!();
    }
}

fn symbolic_permission(p: u32) -> String {
    let mut s = String::new();
    if (p & 0b100) == 0b100 {
        s.push('r');
    } else {
        s.push('-');
    }
    if (p & 0b010) == 0b010 {
        s.push('w');
    } else {
        s.push('-');
    }
    if (p & 0b001) == 0b001 {
        s.push('x');
    } else {
        s.push('-');
    }
    s
}
