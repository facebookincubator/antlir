/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;
use nix::sys::stat::Mode;
use tracing::trace;
use walkdir::DirEntryExt;
use walkdir::WalkDir;

use super::Sendstream;
mod command;
mod tlv;
mod writer;

pub(super) fn build(spec: &Sendstream, out: &Path, layer: &Path) -> Result<()> {
    let rootless = antlir2_rootless::init().context("while initializing rootless")?;
    let canonical_layer = layer.canonicalize()?;
    let mut f = BufWriter::new(std::fs::File::create(out).context("while creating output file")?);

    // Write the magic sentinel and version number. This packager always
    // produces uncompressed v1 sendstreams, and then we conditionally
    // compress or upgrade them to v2 using
    // `antlir/btrfs_send_stream_upgrade` afterwards
    f.write_all(b"btrfs-stream\0")?;
    f.write_all(&1u32.to_le_bytes())?;

    let _root = rootless.escalate()?;

    let subvol = antlir2_btrfs::Subvolume::open(&canonical_layer)
        .with_context(|| format!("while opening subvol {}", canonical_layer.display()))?;
    let info = subvol.info().context("while getting subvol info")?;
    f.write_all(&command::subvol(
        &spec.volume_name,
        info.uuid(),
        info.ctransid(),
    ))?;

    anyhow::ensure!(
        spec.incremental_parent.is_none(),
        "incremental sendstreams not yet supported in rootless"
    );

    // map ino -> relpath so that hardlinks can be detected
    let mut inodes: HashMap<u64, PathBuf> = HashMap::new();

    for entry in WalkDir::new(&canonical_layer) {
        let entry = entry.context("while walking layer")?;
        let relpath = entry.path().strip_prefix(&canonical_layer)?;
        let span = tracing::trace_span!("file", path = relpath.display().to_string());
        let _enter = span.enter();
        trace!("processing dir entry");
        let meta = entry.metadata()?;

        match inodes.entry(entry.ino()) {
            std::collections::hash_map::Entry::Occupied(e) => {
                f.write_all(&command::hardlink(e.get(), relpath))?;
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(relpath.to_owned());
            }
        }

        // we don't need to send a mkdir for the root directory, but all the
        // other things that follow (chown, chmod, etc) should still happen
        if relpath != Path::new("") {
            if meta.file_type().is_dir() {
                f.write_all(&command::mkdir(relpath, entry.ino()))?;
            } else if meta.file_type().is_symlink() {
                let target = std::fs::read_link(entry.path())?;
                f.write_all(&command::symlink(target, relpath, entry.ino()))?;
            } else if meta.file_type().is_file() {
                if meta.file_type().is_block_device() || meta.file_type().is_char_device() {
                    f.write_all(&command::mknod(relpath, meta.mode().into(), meta.rdev()))?;
                } else if meta.file_type().is_fifo() {
                    f.write_all(&command::mkfifo(relpath, entry.ino()))?;
                } else if meta.file_type().is_socket() {
                    f.write_all(&command::mksock(relpath, entry.ino()))?;
                } else {
                    f.write_all(&command::mkfile(relpath, entry.ino()))?;
                    let mut infile =
                        BufReader::new(File::open(entry.path()).with_context(|| {
                            format!("while opening file {}", entry.path().display())
                        })?);
                    // 60k because we need a little bit of space to to store
                    // metadata (so can't use a full 16-bit size), and 4k
                    // boundaries are nice
                    let mut buf = [0u8; 61440];
                    let mut offset = 0;
                    loop {
                        let read = infile.read(&mut buf).with_context(|| {
                            format!("while reading from file {}", entry.path().display())
                        })?;
                        if read == 0 {
                            break;
                        }
                        f.write_all(&command::write(relpath, offset, &buf[..read]))?;
                        offset += read as u64;
                    }
                }
            } else {
                anyhow::bail!("exactly one of is_dir, is_symlink, is_file must be true");
            }
        }

        f.write_all(&command::chown(
            relpath,
            meta.uid().into(),
            meta.gid().into(),
        ))?;
        if !meta.file_type().is_symlink() {
            let mode = Mode::from_bits_truncate(meta.mode());
            f.write_all(&command::chmod(relpath, mode.bits().into()))?;
        }

        for (name, val) in get_xattrs(entry.path())? {
            f.write_all(&command::set_xattr(relpath, name.as_bytes(), val))?;
        }

        let atime = meta.accessed().unwrap_or(UNIX_EPOCH);
        let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
        let ctime = UNIX_EPOCH + Duration::new(meta.ctime() as u64, meta.ctime_nsec() as u32);
        f.write_all(&command::utimes(relpath, atime, mtime, ctime))?;
    }

    f.write_all(&command::end())?;

    Ok(())
}

fn get_xattrs(path: &Path) -> Result<HashMap<OsString, Vec<u8>>> {
    let mut xattrs = HashMap::new();
    for xattr_name in
        xattr::list(path).with_context(|| format!("while listing xattrs on {}", path.display()))?
    {
        let val = xattr::get(path, &xattr_name)
            .with_context(|| {
                format!(
                    "while reading xattr {} on {}",
                    xattr_name.to_string_lossy(),
                    path.display()
                )
            })?
            .with_context(|| {
                format!(
                    "xattr {} on {} disappeared while reading",
                    xattr_name.to_string_lossy(),
                    path.display()
                )
            })?;
        xattrs.insert(xattr_name, val);
    }
    Ok(xattrs)
}
