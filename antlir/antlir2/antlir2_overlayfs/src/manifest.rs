/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! RE/CAS cannot represent certain filesystem metadata (xattrs, device nodes,
//! and ownership to name a few).
//!
//! This module provides functionality to losslessly transform a single
//! antlir2_overlayfs layer to/from a format that RE/CAS can safely record.

use std::ffi::OsStr;
use std::fs::create_dir_all;
use std::fs::OpenOptions;
use std::fs::Permissions;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::fchown;
use std::os::unix::fs::symlink;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use nix::fcntl::OFlag;
use nix::sys::stat::makedev;
use nix::sys::stat::mknod;
use nix::sys::stat::Mode;
use nix::sys::stat::SFlag;
use nix::unistd::mkfifo;
use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use walkdir::WalkDir;
use xattr::FileExt as _;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Manifest {
    entries: Vec<(PathBuf, DirEntry)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct DirEntry {
    uid: u32,
    gid: u32,
    mode: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    xattrs: Vec<Xattr>,
    content: Content,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Xattr {
    #[serde_as(as = "serde_with::hex::Hex")]
    name: Vec<u8>,
    #[serde_as(as = "serde_with::hex::Hex")]
    value: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Content {
    RegularFile,
    Directory,
    Symlink(PathBuf),
    Device(u64),
    Fifo,
    /// Whiteout is an overlayfs concept that means a directory entry was
    /// deleted in that layer
    Whiteout,
}

impl Manifest {
    pub(crate) fn from_directory(dir: &Path) -> Result<Self> {
        let mut entries = Vec::new();
        for entry in WalkDir::new(dir) {
            let entry = entry?;
            let relpath = entry
                .path()
                .strip_prefix(dir)
                .context("path not under root")?;

            let metadata = entry
                .metadata()
                .with_context(|| format!("while statting '{}'", entry.path().display()))?;
            let ft = metadata.file_type();
            if ft.is_char_device() && metadata.rdev() == 0 {
                entries.push((
                    relpath.to_owned(),
                    DirEntry {
                        uid: metadata.uid(),
                        gid: metadata.gid(),
                        mode: metadata.mode(),
                        xattrs: Vec::new(),
                        content: Content::Whiteout,
                    },
                ));
                continue;
            }
            if ft.is_fifo() {
                entries.push((
                    relpath.to_owned(),
                    DirEntry {
                        uid: metadata.uid(),
                        gid: metadata.gid(),
                        mode: metadata.mode(),
                        xattrs: Vec::new(),
                        content: Content::Fifo,
                    },
                ));
                continue;
            }
            if ft.is_socket() {
                // There isn't a way for us to really recreate these in any
                // meaningful way. They should not be left in image layers
                // anyway. If there ever is a use case for this, we can
                // re-evaluate and attempt to support it (or, more likely, strip
                // them from the layer).
                bail!("socket files should not be left in image");
            }

            let content = if ft.is_dir() {
                Content::Directory
            } else if ft.is_symlink() {
                Content::Symlink(entry.path().read_link().with_context(|| {
                    format!(
                        "while getting symlink target of '{}'",
                        entry.path().display()
                    )
                })?)
            } else if ft.is_block_device() || ft.is_char_device() {
                Content::Device(metadata.rdev())
            } else if ft.is_file() {
                Content::RegularFile
            } else {
                bail!(
                    "'{}' is not a regular file, directory or device",
                    entry.path().display()
                )
            };

            let mut xattrs = Vec::new();

            match OpenOptions::new()
                .read(true)
                .custom_flags(nix::libc::O_NOFOLLOW)
                .open(entry.path())
            {
                Ok(f) => {
                    for name in f.list_xattr().with_context(|| {
                        format!("while listing xattrs on '{}'", entry.path().display())
                    })? {
                        if let Ok(Some(val)) = f.get_xattr(&name) {
                            xattrs.push(Xattr {
                                name: name.into_vec(),
                                value: val,
                            });
                        }
                    }
                    Ok(())
                }
                Err(e) => match e.kind() {
                    std::io::ErrorKind::FilesystemLoop => Ok(()),
                    _ => Err(anyhow::Error::from(e)
                        .context(format!("while opening '{}'", entry.path().display()))),
                },
            }?;

            entries.push((
                relpath.to_owned(),
                DirEntry {
                    uid: metadata.uid(),
                    gid: metadata.gid(),
                    mode: metadata.mode(),
                    xattrs,
                    content,
                },
            ));
        }
        Ok(Self { entries })
    }

    pub(crate) fn fix_directory(&self, root: &Path) -> Result<()> {
        for (relpath, entry) in &self.entries {
            let dst = if relpath == Path::new("") {
                root.to_owned()
            } else {
                root.join(relpath)
            };
            match &entry.content {
                Content::Directory | Content::RegularFile => {
                    let mut opts = OpenOptions::new();
                    opts.read(true);
                    opts.custom_flags(OFlag::O_NOFOLLOW.bits());
                    if entry.content == Content::Directory {
                        // If the directory is empty, it may not have been
                        // created yet
                        create_dir_all(&dst)?;
                        opts.custom_flags(OFlag::O_DIRECTORY.bits());
                    }
                    let fd = opts
                        .open(&dst)
                        .with_context(|| format!("while opening '{}'", dst.display()))?;
                    fchown(&fd, Some(entry.uid), Some(entry.gid))
                        .with_context(|| format!("while chowning '{}'", dst.display()))?;
                    for xattr in &entry.xattrs {
                        fd.set_xattr(OsStr::from_bytes(&xattr.name), &xattr.value)
                            .with_context(|| {
                                format!(
                                    "while setting xattr '{}' on '{}'",
                                    String::from_utf8_lossy(&xattr.name),
                                    dst.display(),
                                )
                            })?;
                    }
                    fd.set_permissions(Permissions::from_mode(entry.mode))
                        .with_context(|| format!("while chmodding '{}'", dst.display()))?;
                }
                Content::Symlink(target) => {
                    symlink(target, &dst).with_context(|| {
                        format!(
                            "while symlinking '{}' -> '{}'",
                            dst.display(),
                            target.display()
                        )
                    })?;
                }
                Content::Device(rdev) => {
                    mknod(
                        &dst,
                        SFlag::from_bits_truncate(entry.mode),
                        Mode::from_bits_truncate(entry.mode),
                        *rdev,
                    )
                    .with_context(|| format!("while making device node '{}'", dst.display()))?;
                }
                Content::Fifo => {
                    mkfifo(&dst, Mode::from_bits_truncate(entry.mode))
                        .with_context(|| format!("while making fifo '{}'", dst.display()))?;
                }
                Content::Whiteout => {
                    mknod(&dst, SFlag::S_IFCHR, Mode::empty(), makedev(0, 0))
                        .with_context(|| format!("while making whiteout '{}'", dst.display()))?;
                }
            };
        }
        Ok(())
    }
}
