/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::path::PathBuf;

use antlir2_change_stream::Iter;
use antlir2_change_stream::Operation;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use nix::sys::stat::major;
use nix::sys::stat::minor;
use nix::sys::stat::SFlag;
use tar::Builder;
use tar::EntryType;
use tar::Header;

#[derive(Parser, Debug)]
struct Args {
    #[clap(long)]
    parent: Option<PathBuf>,
    #[clap(long)]
    child: PathBuf,
    #[clap(long)]
    out: PathBuf,
    #[clap(long)]
    rootless: bool,
}

struct Entry {
    header: Header,
    contents: Contents,
    extensions: Vec<(String, Vec<u8>)>,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            header: Header::new_ustar(),
            contents: Contents::Unset,
            extensions: Vec::new(),
        }
    }
}

enum Contents {
    Unset,
    Link(PathBuf),
    File(File),
    Whiteout,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.rootless {
        antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
    }

    let stream: Iter<File> = match &args.parent {
        Some(parent) => Iter::diff(parent, &args.child)?,
        None => Iter::from_empty(&args.child)?,
    };
    let mut entries: BTreeMap<PathBuf, Entry> = BTreeMap::new();
    for change in stream {
        let change = change?;
        let path = change.path().to_owned();
        match change.into_operation() {
            Operation::Create { mode } => {
                let header = &mut entries.entry(path).or_default().header;
                header.set_mode(mode);
                header.set_entry_type(EntryType::Regular);
            }
            Operation::Mkdir { mode } => {
                let header = &mut entries.entry(path).or_default().header;
                header.set_mode(mode);
                header.set_entry_type(EntryType::Directory);
            }
            Operation::Mkfifo { mode } => {
                let header = &mut entries.entry(path).or_default().header;
                header.set_mode(mode);
                header.set_entry_type(EntryType::Fifo);
            }
            Operation::Mknod { rdev, mode } => {
                let header = &mut entries.entry(path).or_default().header;
                header.set_mode(mode);
                let sflag = SFlag::from_bits_truncate(mode);
                header.set_entry_type(if sflag.contains(SFlag::S_IFBLK) {
                    EntryType::Block
                } else {
                    EntryType::Char
                });
                header.set_device_major(major(rdev) as u32)?;
                header.set_device_minor(minor(rdev) as u32)?;
            }
            Operation::Chmod { mode } => {
                let header = &mut entries.entry(path).or_default().header;
                header.set_mode(mode);
            }
            Operation::Chown { uid, gid } => {
                let header = &mut entries.entry(path).or_default().header;
                header.set_uid(uid as u64);
                header.set_gid(gid as u64);
            }
            Operation::SetTimes { atime: _, mtime: _ } => {
                // timestamps make things very non-reproducible
            }
            Operation::HardLink { target } => {
                let entry = entries.entry(path).or_default();
                entry.header.set_entry_type(EntryType::Link);
                entry.contents = Contents::Link(target.to_owned());
            }
            Operation::Symlink { target } => {
                let entry = entries.entry(path).or_default();
                entry.header.set_entry_type(EntryType::Symlink);
                entry.contents = Contents::Link(target.to_owned());
            }
            Operation::Rename { to: _ } => {
                // just ensure an entry exists, which will end up sending the
                // full contents, since there is no way to represent a rename in
                // the layer tar
                entries.entry(path).or_default();
            }
            Operation::Contents { contents } => {
                let entry = entries.entry(path).or_default();
                entry.contents = Contents::File(contents);
            }
            Operation::RemoveXattr { .. } => {
                // just ensure an entry exists, which will end up sending the
                // full contents
                entries.entry(path).or_default();
            }
            Operation::SetXattr { name, value } => {
                let entry = entries.entry(path).or_default();
                let mut key = "SCHILY.xattr.".to_owned();
                key.push_str(
                    name.to_str()
                        .with_context(|| format!("xattr name '{name:?}' is not valid UTF-8"))?,
                );
                entry.extensions.push((key, value))
            }
            // Removals are represented with special whiteout marker files
            Operation::Unlink | Operation::Rmdir => {
                let mut wh_name = OsString::from(".wh.");
                wh_name.push(path.file_name().expect("root dir cannot be deleted"));
                let wh_path = path.parent().unwrap_or(Path::new("")).join(wh_name);
                entries
                    .entry(wh_path)
                    .or_insert_with(Default::default)
                    .contents = Contents::Whiteout;
            }
        }
    }

    let mut builder = Builder::new(BufWriter::new(File::create(&args.out)?));
    for (path, mut entry) in entries {
        if path == Path::new("") {
            continue;
        }
        // PAX extensions go ahead of the full entry header
        entry.extensions.sort();
        builder.append_pax_extensions(
            entry
                .extensions
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_slice())),
        )?;
        // Timestamps make things non-deterministic even if everything else is
        // 100% equal. To get around this (and to preempty any bugs from tools
        // that don't tolerate 0 timestamps very well), choose an arbitrary time
        // of February 4 2004, the initial launch of thefacebook.com
        entry.header.set_mtime(1075852800);
        match entry.contents {
            Contents::Link(target) => {
                builder.append_link(&mut entry.header, path, target)?;
            }
            Contents::File(mut f) => {
                builder.append_file(path, &mut f)?;
            }
            Contents::Whiteout => {
                builder.append_data(&mut entry.header, path, std::io::empty())?;
            }
            Contents::Unset => {
                // Metadata only change, but the OCI spec says that any change
                // must send the entire contents, so open it up from the child
                // layer.
                let meta = std::fs::symlink_metadata(args.child.join(&path))?;
                if meta.is_file() {
                    let mut f = File::open(args.child.join(&path))?;
                    builder.append_file(path, &mut f)?;
                } else if meta.is_dir() {
                    entry.header.set_entry_type(EntryType::Directory);
                    builder.append_data(&mut entry.header, path, std::io::empty())?;
                } else if meta.is_symlink() {
                    entry.header.set_entry_type(EntryType::Symlink);
                    let target = std::fs::read_link(args.child.join(&path))?;
                    builder.append_link(&mut entry.header, path, target)?;
                } else {
                    bail!(
                        "not sure what to do with unset contents on filetype {:?}",
                        meta.file_type(),
                    );
                }
            }
        }
    }
    Ok(())
}
