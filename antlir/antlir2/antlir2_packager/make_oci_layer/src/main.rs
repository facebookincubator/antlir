/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;

use antlir2_change_stream::Iter;
use antlir2_change_stream::Operation;
use anyhow::bail;
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
}

struct Entry {
    header: Header,
    contents: Contents,
    extensions: Vec<(Header, Vec<u8>)>,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            header: Header::new_gnu(),
            contents: Contents::Unset,
            extensions: Vec::new(),
        }
    }
}

enum Contents {
    Unset,
    Link(PathBuf),
    File(BufReader<File>),
    Whiteout,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let stream: Iter<BufReader<File>> = match &args.parent {
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
            Operation::SetTimes { atime: _, mtime } => {
                let header = &mut entries.entry(path).or_default().header;
                header.set_mtime(mtime.elapsed()?.as_secs());
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
                let mut ext = Header::new_ustar();
                let mut kv = b"SCHILY.xattr.".to_vec();
                kv.extend(name.as_bytes());
                kv.push(b'=');
                kv.extend(value);
                kv.push(b'\n');
                let mut data = Vec::new();
                write!(&mut data, "{} ", kv.len())?;
                data.extend(kv);
                ext.set_entry_type(EntryType::XHeader);
                ext.set_size(data.len() as u64);
                ext.set_cksum();
                entry.extensions.push((ext, data));
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
        match entry.contents {
            Contents::Link(target) => {
                builder.append_link(&mut entry.header, path, target)?;
            }
            Contents::File(f) => {
                entry.header.set_size(f.get_ref().metadata()?.len());
                builder.append_data(&mut entry.header, path, f)?;
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
                    let mut f = BufReader::new(File::open(args.child.join(&path))?);
                    entry.header.set_entry_type(EntryType::Regular);
                    entry.header.set_size(f.get_ref().metadata()?.len());
                    builder.append_data(&mut entry.header, path, &mut f)?;
                } else if meta.is_dir() {
                    entry.header.set_entry_type(EntryType::Directory);
                    builder.append_data(&mut entry.header, path, std::io::empty())?;
                } else if meta.is_symlink() {
                    entry.header.set_entry_type(EntryType::Symlink);
                    let target = std::fs::read_link(args.child.join(&path))?;
                    builder.append_data(&mut entry.header, path, target.as_os_str().as_bytes())?;
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
