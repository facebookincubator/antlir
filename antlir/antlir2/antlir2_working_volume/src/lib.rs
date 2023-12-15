/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Seek;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_btrfs::Subvolume;
use anyhow::Context;
use fs2::FileExt as _;
use nix::dir::Dir;
use nix::errno::Errno;
use nix::fcntl::flock;
use nix::fcntl::FlockArg;
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;
use serde::Deserialize;
use serde::Serialize;
use tracing::trace;
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Btrfs(#[from] antlir2_btrfs::Error),
    #[error("failed to add redirect: {error}")]
    AddRedirect { error: String },
    #[error("failed to create working volume")]
    CreateWorkingVolume(std::io::Error),
    #[error("failed to check eden presence")]
    CheckEden(std::io::Error),
    #[error("error tracking subvolumes: {0:#?}")]
    Tracking(anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct WorkingVolume {
    path: PathBuf,
}

impl WorkingVolume {
    /// Ensure this [WorkingVolume] exists and is set up correctly.
    pub fn ensure(path: PathBuf) -> Result<Self> {
        // If we're on Eden, create a new redirection
        // https://www.internalfb.com/intern/wiki/EdenFS/detecting-an-eden-mount/#on-linux-and-macos
        match Dir::open(".eden", OFlag::O_RDONLY, Mode::empty()) {
            Ok(dir) => {
                // There seems to be some racy behavior with eden adding
                // redirects, take an exclusive lock before adding
                flock(dir.as_raw_fd(), FlockArg::LockExclusive).map_err(std::io::Error::from)?;
                if !path.exists() {
                    let res = Command::new("eden")
                        .env("EDENFSCTL_ONLY_RUST", "1")
                        .arg("redirect")
                        .arg("add")
                        .arg(&path)
                        .arg("bind")
                        .output()
                        .map_err(|e| Error::AddRedirect {
                            error: e.to_string(),
                        })?;
                    if !res.status.success() {
                        return Err(Error::AddRedirect {
                            error: String::from_utf8_lossy(&res.stderr).into_owned(),
                        });
                    }
                }
                Ok(Self { path })
            }
            Err(e) => match e {
                Errno::ENOENT => {
                    trace!("no .eden: {e:?}");
                    if let Err(e) = std::fs::create_dir(&path) {
                        match e.kind() {
                            ErrorKind::AlreadyExists => Ok(Self { path }),
                            _ => Err(Error::CreateWorkingVolume(e)),
                        }
                    } else {
                        Ok(Self { path })
                    }
                }
                _ => Err(Error::CheckEden(std::io::Error::from(e))),
            },
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Provide a new (non-existent) path for an image build to put its result
    /// into.
    pub fn allocate_new_path(&self) -> Result<PathBuf> {
        Ok(self.path.join(Uuid::new_v4().simple().to_string()))
    }

    fn tracked_subvols(&self) -> Result<SubvolsFile> {
        SubvolsFile::open(&self.path.join("subvols.json"))
    }

    /// Increment a refcount using a file in buck-out to prevent the path in
    /// this [WorkingVolume] from being garbage collected
    pub fn keep_path_alive(&self, allocated_path: &Path, buck_out_path: &Path) -> Result<()> {
        let mut subvols = self.tracked_subvols()?;
        subvols.track(allocated_path.to_owned(), buck_out_path.to_owned());
        subvols.serialize().map_err(Error::Tracking)?;
        Ok(())
    }

    /// Delete outputs that are no longer referenced by buck artifacts, but are
    /// still stored in this [WorkingVolume].
    pub fn collect_garbage(&self) -> anyhow::Result<()> {
        if no_gc() {
            trace!("gc is disabled");
            return Ok(());
        }
        let mut subvols = self.tracked_subvols()?;
        for subvol in subvols.deletable_subvols() {
            if delete(&subvol).is_ok() {
                subvols.untrack(&subvol);
            }
        }
        subvols
            .serialize()
            .context("while serializing subvols.json")?;
        Ok(())
    }
}

#[tracing::instrument]
fn delete(path: &Path) -> Result<()> {
    match Subvolume::open(path) {
        Ok(subvol) => {
            if let Err((_subvol, e)) = subvol.delete() {
                warn!("failed deleting subvol: {e}");
                Err(e.into())
            } else {
                Ok(())
            }
        }
        Err(e) => {
            warn!("failed opening subvol to be deleted: {e}");
            if let Err(e) = std::fs::remove_dir_all(path) {
                warn!("failed to rmdir: {e}");
                if let Err(e) = std::fs::remove_file(path) {
                    warn!("failed to remove: {e}");
                    Err(e.into())
                } else {
                    Ok(())
                }
            } else {
                Ok(())
            }
        }
    }
}

/// Struct that is serialized to antlir2-out/subvols.json and used to keep track
/// of where subvolumes are referenced.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
struct Subvols {
    #[serde(default)]
    subvols: BTreeMap<PathBuf, Vec<PathBuf>>,
}

impl Subvols {
    fn deletable_subvols(&self) -> Vec<PathBuf> {
        let mut result = Vec::new();
        for (subvol, symlinks) in &self.subvols {
            if let Ok(subvol_canonical) = subvol.canonicalize() {
                // If none of the symlinks exist and point to the subvol, then it
                // can safely be deleted.
                let symlink_targets: HashSet<_> = symlinks
                    .iter()
                    .filter_map(|symlink| symlink.canonicalize().ok())
                    .collect();
                if symlink_targets.is_empty() || !symlink_targets.contains(&subvol_canonical) {
                    result.push(subvol.clone());
                }
            }
        }
        result
    }

    fn track(&mut self, subvol: PathBuf, buck_out: PathBuf) {
        self.subvols.entry(subvol).or_default().push(buck_out);
    }

    fn untrack(&mut self, subvol: &Path) {
        self.subvols.remove(subvol);
    }
}

struct SubvolsFile {
    subvols: Subvols,
    file: File,
}

impl SubvolsFile {
    fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .context(format!("while opening subvol.json: {}", path.display()))
            .map_err(Error::Tracking)?;
        file.lock_exclusive()
            .context(format!("while locking subvols.json: {}", path.display()))
            .map_err(Error::Tracking)?;
        let subvols: Subvols = if file
            .metadata()
            .context(format!("while statting subvols.json: {}", path.display()))
            .map_err(Error::Tracking)?
            .len()
            == 0
        {
            Default::default()
        } else {
            serde_json::from_reader(&mut file)
                .context("while reading subvols.json")
                .map_err(Error::Tracking)?
        };
        Ok(Self { subvols, file })
    }

    fn track(&mut self, subvol: PathBuf, buck_out: PathBuf) {
        self.subvols.track(subvol, buck_out)
    }

    fn untrack(&mut self, subvol: &Path) {
        self.subvols.untrack(subvol)
    }

    fn deletable_subvols(&self) -> Vec<PathBuf> {
        self.subvols.deletable_subvols()
    }

    fn serialize(mut self) -> anyhow::Result<()> {
        self.file.rewind().context("while rewinding")?;
        self.file.set_len(0).context("while truncating")?;
        serde_json::to_writer_pretty(&mut self.file, &self.subvols)
            .context("while writing subvols.json")?;
        Ok(())
    }
}

/// If we're on CI we're never even going to re-build the same thing twice
/// anyway, so there's no point in keeping the refcounts around.
fn no_gc() -> bool {
    std::env::var_os("SANDCASTLE_INSTANCE_ID").is_some()
}
