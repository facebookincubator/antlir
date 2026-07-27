/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
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

use antlir2_btrfs::Subvolume;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use nix::sys::stat::Mode;
use tracing::trace;
use uuid::Uuid;
use walkdir::DirEntryExt;
use walkdir::WalkDir;

use super::Sendstream;
mod command;
mod tlv;
mod writer;

const BLAKE3_KEY: &str = "74dbd000-062c-498b-92fc-3e4b7efdcab4";

pub(super) fn build(spec: &Sendstream, out: &Path, layer: &Path) -> Result<()> {
    let rootless = antlir2_rootless::init().context("while initializing rootless")?;
    let canonical_layer = layer.canonicalize()?;

    let mut f = BufWriter::new(File::create(out).context("while creating output file")?);

    // Write the magic sentinel and version number. This packager always
    // produces uncompressed v1 sendstreams, and lets the sendstream-upgrade
    // command upgrade it to v2
    f.write_all(b"btrfs-stream\0")?;
    f.write_all(&1u32.to_le_bytes())?;

    let _root = rootless.escalate()?;

    let subvol = antlir2_btrfs::Subvolume::open(&canonical_layer)
        .with_context(|| format!("while opening subvol {}", canonical_layer.display()))?;
    let info = subvol.info().context("while getting subvol info")?;

    // NOTE on uuids: each subvolume has its own true (unique) uuid, but we
    // want to put the "received uuid" (if it exists) in the sendstream so that
    // they can be accurately identified by the receiver for use with
    // incremental send/receive.
    if let Some(parent) = &spec.incremental_parent {
        let parent_info = antlir2_btrfs::Subvolume::open(parent)
            .with_context(|| format!("while opening parent subvol {}", parent.display()))?
            .info()
            .with_context(|| format!("while getting info of parent subvol {}", parent.display()))?;
        f.write_all(&command::snapshot(
            &spec.volume_name,
            info.uuid(),
            info.ctransid(),
            parent_info.received_uuid().unwrap_or(parent_info.uuid()),
            parent_info.ctransid(),
        ))?;
    } else {
        f.write_all(&command::subvol(
            &spec.volume_name,
            info.received_uuid().unwrap_or(info.uuid()),
            info.ctransid(),
        ))?;
    }

    // map ino -> relpath so that hardlinks can be detected
    let mut inodes: HashMap<(u64, u64), PathBuf> = HashMap::new();
    // keep track of relpaths which are seen in this subvol in case they
    // need to be deleted from the parent
    let mut present_relpaths: HashSet<PathBuf> = HashSet::new();

    for entry in WalkDir::new(&canonical_layer) {
        let entry = entry.context("while walking layer")?;
        let relpath = entry.path().strip_prefix(&canonical_layer)?;
        present_relpaths.insert(relpath.to_owned());
        let span = tracing::trace_span!("file", path = relpath.display().to_string());
        let _enter = span.enter();
        trace!("processing dir entry");
        let meta = entry.metadata()?;

        // Recursive subvolumes are tricky for antlir to manage - snapshots are
        // not recursive for example and the traces of the recursive subvolumes
        // are left as directories that appear to all have ino=2.
        // But rather than handle subvolumes specially, just skip all
        // directories when doing this can-be-hardlinked check, since
        // directories cannot be hardlinked
        if !meta.is_dir() {
            match inodes.entry((meta.dev(), meta.ino())) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    if let Some(parent) = &spec.incremental_parent {
                        let parent_path = parent.join(relpath);
                        match parent_path.symlink_metadata() {
                            Ok(parent_meta) => {
                                // hardlink already exists in child, skip
                                // NOTE: the 'st_dev' is intentionally not
                                // checked here, as subvolumes have artificially
                                // different 'st_dev' values, even when
                                // snapshotted
                                if parent_meta.ino() == meta.ino() {
                                    continue;
                                }
                                // otherwise, continue on to create it below
                            }
                            // new file in child, fallthrough to non-incremental path
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => {
                                return Err(
                                    Error::from(e).context("while getting parent file metadata")
                                );
                            }
                        }
                    }
                    let tmp = Uuid::new_v4().to_string();
                    f.write_all(&command::hardlink(e.get(), &tmp))?;
                    f.write_all(&command::rename(tmp, relpath))?;
                    continue;
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(relpath.to_owned());
                }
            }
        }

        if let Some(parent) = &spec.incremental_parent {
            let parent_path = parent.join(relpath);
            match parent_path.symlink_metadata() {
                Ok(parent_meta) => {
                    if meta.is_dir() {
                        // If this is a directory, the only thing we need to
                        // send over are metadata updates
                        if (meta.uid() != parent_meta.uid()) || (meta.gid() != parent_meta.gid()) {
                            f.write_all(&command::chown(
                                relpath,
                                meta.uid() as u64,
                                meta.gid() as u64,
                            ))?;
                        }
                        if meta.mode() != parent_meta.mode() {
                            f.write_all(&command::chmod(relpath, meta.mode() as u64))?;
                        }
                        let mut parent_xattrs: HashMap<_, _> = xattr::list(&parent_path)?
                            .map(|name| {
                                xattr::get(&parent_path, &name)
                                    .map(|val| (name, val))
                                    .context("while getting xattr on parent")
                            })
                            .collect::<Result<_>>()?;
                        let self_xattrs: HashMap<_, _> = xattr::list(entry.path())?
                            .map(|name| {
                                xattr::get(entry.path(), &name)
                                    .map(|val| (name, val))
                                    .context("while getting xattr on self")
                            })
                            .collect::<Result<_>>()?;
                        for (name, val) in self_xattrs
                            .into_iter()
                            .filter_map(|(n, v)| v.map(|v| (n, v)))
                        {
                            if let Some(parent_val) = parent_xattrs.remove(&name) {
                                if Some(&val) != parent_val.as_ref() {
                                    f.write_all(&command::set_xattr(
                                        relpath,
                                        name.as_bytes(),
                                        val,
                                    ))?;
                                }
                            }
                        }
                        for name in parent_xattrs.keys() {
                            f.write_all(&command::rm_xattr(relpath, name.as_bytes()))?;
                        }
                        continue;
                    }

                    if meta.is_file() {
                        let file_contents_changed = if parent_meta.len() != meta.len() {
                            true
                        } else {
                            let parent_file =
                                BufReader::new(File::open(&parent_path).with_context(|| {
                                    format!("while opening file {}", parent_path.display())
                                })?);
                            let mut hasher = blake3::Hasher::new_derive_key(BLAKE3_KEY);
                            hasher.update_reader(parent_file).with_context(|| {
                                format!("while hashing file {}", parent_path.display())
                            })?;
                            let parent_hash = hasher.finalize();
                            let infile =
                                BufReader::new(File::open(entry.path()).with_context(|| {
                                    format!("while opening file {}", entry.path().display())
                                })?);
                            let mut hasher = blake3::Hasher::new_derive_key(BLAKE3_KEY);
                            hasher.update_reader(infile).with_context(|| {
                                format!("while hashing file {}", entry.path().display())
                            })?;
                            let new_hash = hasher.finalize();
                            parent_hash != new_hash
                        };

                        if file_contents_changed {
                            // TODO: support more efficient updates on
                            // append-only files (where len > parent_len but
                            // hash(parent[..parent_len]) == hash(file[..parent_len]))
                            f.write_all(&command::truncate(relpath, meta.size()))?;
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
                                    format!("1while reading from file {}", entry.path().display())
                                })?;
                                if read == 0 {
                                    break;
                                }
                                f.write_all(&command::write(relpath, offset, &buf[..read]))?;
                                offset += read as u64;
                            }
                        }
                    }

                    if meta.is_symlink() {
                        let original_target =
                            std::fs::read_link(&parent_path).with_context(|| {
                                format!("while reading link target of {}", parent_path.display())
                            })?;
                        let new_target = std::fs::read_link(entry.path()).with_context(|| {
                            format!("while reading link target of {}", entry.path().display())
                        })?;
                        if new_target != original_target {
                            f.write_all(&command::unlink(relpath))?;
                            f.write_all(&command::symlink(new_target, relpath, entry.ino()))?;
                            f.write_all(&command::chown(
                                relpath,
                                meta.uid().into(),
                                meta.gid().into(),
                            ))?;
                        }
                    }

                    if parent_meta.uid() != meta.uid() || parent_meta.gid() != meta.gid() {
                        f.write_all(&command::chown(
                            relpath,
                            meta.uid().into(),
                            meta.gid().into(),
                        ))?;
                    }
                    if !meta.file_type().is_symlink() && (parent_meta.mode() != meta.mode()) {
                        let mode = Mode::from_bits_truncate(meta.mode());
                        f.write_all(&command::chmod(relpath, mode.bits().into()))?;
                    }
                    let mut parent_xattrs = get_xattrs(&parent_path)?;
                    let self_xattrs = get_xattrs(entry.path())?;
                    for (name, val) in self_xattrs.into_iter().collect::<HashMap<_, _>>() {
                        match parent_xattrs.remove(&name) {
                            Some(parent_val) => {
                                // skip this xattr if the value has not changed
                                if val == parent_val {
                                    continue;
                                }
                            }
                            None => {}
                        }
                        f.write_all(&command::set_xattr(relpath, name.as_bytes(), val))?;
                    }
                    for del in parent_xattrs.into_keys() {
                        f.write_all(&command::rm_xattr(relpath, del.as_bytes()))?;
                    }
                    continue;
                }
                // completely new file, fallthrough to non-incremental path
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(Error::from(e).context("while getting parent file metadata"));
                }
            };
        }

        // we don't need to send a mkdir for the root directory, but all the
        // other things that follow (chown, chmod, etc) should still happen
        if relpath != Path::new("") {
            if meta.file_type().is_dir() {
                f.write_all(&command::mkdir(relpath, entry.ino()))?;
            } else if meta.file_type().is_symlink() {
                let target = std::fs::read_link(entry.path())?;
                // create symlink at a temporary path, then rename it to the
                // real destination - this matches the in-kernel implementation
                let tmp = Uuid::new_v4().to_string();
                f.write_all(&command::symlink(target, &tmp, entry.ino()))?;
                f.write_all(&command::rename(tmp, relpath))?;
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

    if let Some(parent) = &spec.incremental_parent {
        for entry in WalkDir::new(parent).contents_first(true) {
            let entry = entry.context("while walking layer")?;
            let relpath = entry.path().strip_prefix(parent)?;
            if !present_relpaths.contains(relpath) {
                if entry.file_type().is_dir() {
                    f.write_all(&command::rmdir(relpath))?;
                } else {
                    f.write_all(&command::unlink(relpath))?;
                }
            }
        }
    }

    f.write_all(&command::end())?;
    f.flush().context("while flushing BufWriter")?;
    f.into_inner()
        .context("while dropping BufWriter")?
        .sync_all()
        .context("while syncing output file")?;
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
