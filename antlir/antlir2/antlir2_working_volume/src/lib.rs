/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::time::SystemTime;

use antlir2_btrfs::DeleteFlags;
use antlir2_btrfs::Subvolume;
use nix::dir::Dir;
use nix::errno::Errno;
use nix::fcntl::flock;
use nix::fcntl::FlockArg;
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;
use serde::Deserialize;
use tracing::trace;
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse redirects from '{text}': {error:?}")]
    ParseRedirects {
        text: String,
        error: Option<serde_json::Error>,
    },
    #[error("failed to add redirect: {error}")]
    AddRedirect { error: String },
    #[error("failed to create working volume")]
    CreateWorkingVolume(std::io::Error),
    #[error("failed to check eden presence")]
    CheckEden(std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct WorkingVolume {
    path: PathBuf,
    eden: Option<EdenInfo>,
}

#[derive(Debug, Clone)]
struct EdenInfo {
    repo_root: PathBuf,
    redirections: Vec<EdenRedirection>,
}

#[derive(Debug, Clone, Deserialize)]
struct EdenRedirection {
    repo_path: PathBuf,
    target: Option<PathBuf>,
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
                let res = Command::new("eden")
                    .env("EDENFSCTL_ONLY_RUST", "1")
                    .arg("redirect")
                    .arg("list")
                    .arg("--json")
                    .output()?;
                let stdout =
                    std::str::from_utf8(&res.stdout).map_err(|_| Error::ParseRedirects {
                        text: "<not utf8>".to_owned(),
                        error: None,
                    })?;
                let redirections =
                    serde_json::from_str(stdout).map_err(|e| Error::ParseRedirects {
                        text: stdout.to_owned(),
                        error: Some(e),
                    })?;
                trace!("parsed eden redirections: {redirections:?}");
                let repo_root = std::fs::read_link(".eden/root")?;
                Ok(Self {
                    path,
                    eden: Some(EdenInfo {
                        repo_root,
                        redirections,
                    }),
                })
            }
            Err(e) => match e {
                Errno::ENOENT => {
                    trace!("no .eden: {e:?}");
                    if let Err(e) = std::fs::create_dir(&path) {
                        match e.kind() {
                            ErrorKind::AlreadyExists => Ok(Self { path, eden: None }),
                            _ => Err(Error::CreateWorkingVolume(e)),
                        }
                    } else {
                        Ok(Self { path, eden: None })
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

    fn keepalive_path(path: &Path) -> PathBuf {
        let mut filename = path.file_name().expect("cannot be /").to_owned();
        filename.push(".keepalive");
        path.parent().expect("cannot be /").join(filename)
    }

    /// Increment a refcount using a file in buck-out to prevent the path in
    /// this [WorkingVolume] from being garbage collected
    pub fn keep_path_alive(&self, allocated_path: &Path, buck_out_path: &Path) -> Result<()> {
        if no_gc() {
            trace!("gc is disabled");
            File::create(buck_out_path)?;
            return Ok(());
        }
        let full_keepalive_path = self.resolve_redirection(&Self::keepalive_path(allocated_path));
        File::create(&full_keepalive_path)?;

        let buck_parent = buck_out_path
            .parent()
            .expect("cannot be /")
            .canonicalize()?;

        // On Eden, we need to resolve redirects because hardlinks are not
        // allowed across bind mounts (even within the same underlying
        // filesystem). On non-Eden, buck-out and antlir2-out will be being
        // created on the same filesystem without any bind-mount trickery.
        let full_buck_out_path = self
            .resolve_redirection(&buck_parent)
            .join(buck_out_path.file_name().expect("cannot be /"));
        std::fs::hard_link(full_keepalive_path, full_buck_out_path)?;
        Ok(())
    }

    /// If path is under an eden redirection, resolve it to be under the target.
    fn resolve_redirection(&self, path: &Path) -> PathBuf {
        match &self.eden {
            Some(eden) => {
                for (repo_path, target) in eden.redirections.iter().filter_map(|redir| {
                    redir
                        .target
                        .as_ref()
                        .map(|target| (&redir.repo_path, target))
                }) {
                    for prefix in [repo_path, eden.repo_root.join(repo_path).as_path()] {
                        if let Ok(relpath) = path.strip_prefix(prefix) {
                            return target.join(relpath);
                        }
                    }
                }
                path.to_owned()
            }
            None => path.to_owned(),
        }
    }

    /// Delete outputs that are no longer referenced by buck artifacts, but are
    /// still stored in this [WorkingVolume].
    pub fn collect_garbage(&self) -> anyhow::Result<()> {
        if no_gc() {
            trace!("gc is disabled");
            return Ok(());
        }
        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            if entry.path().is_dir() {
                // skip outputs that are very new to avoid any race condition
                // between creating the output and setting the keepalive
                if let Ok(mtime) = entry.metadata().and_then(|meta| meta.modified()) {
                    let elapsed = SystemTime::now().duration_since(mtime)?;
                    if elapsed < Duration::from_secs(5 * 60) {
                        continue;
                    }
                }

                let keepalive_path = Self::keepalive_path(&entry.path());
                let delete = match keepalive_path.metadata() {
                    Ok(meta) => Ok(meta.nlink() <= 1),
                    Err(e) => match e.kind() {
                        ErrorKind::NotFound => Ok(true),
                        _ => Err(e),
                    },
                }?;
                if delete {
                    trace!("{} is no longer referenced", entry.path().display());
                    try_delete_subvol(&entry.path());
                    if let Err(e) = std::fs::remove_file(&keepalive_path) {
                        warn!("failed to remove keepalive file: {e}");
                    }
                }
            }
        }
        Ok(())
    }
}

#[tracing::instrument]
fn try_delete_subvol(path: &Path) {
    match Subvolume::open(path) {
        Ok(subvol) => {
            if let Err((_subvol, e)) = subvol.delete(DeleteFlags::RECURSIVE) {
                warn!("failed deleting subvol: {e}")
            }
        }
        Err(e) => {
            warn!("failed opening subvol to be deleted: {e}")
        }
    }
}

/// We use hardlink refcounting for a garbage collection mechanism. But if we're
/// on CI we're never even going to re-build the same thing twice anyway, so
/// there's no point in keeping the refcounts around.
/// Ideally we would keep doing this anyway, but there is some weird bind mount
/// setup on CI that makes refcounting not work, so just ignore it (if it's
/// never going to be used anyway!)
fn no_gc() -> bool {
    std::env::var_os("SANDCASTLE_INSTANCE_ID").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_redirection() {
        let wv = WorkingVolume {
            path: PathBuf::from("antlir2-out"),
            eden: Some(EdenInfo {
                repo_root: "/path/to/repo".into(),
                redirections: vec![
                    EdenRedirection {
                        repo_path: "buck-out".into(),
                        target: Some("/other/buck-out".into()),
                    },
                    EdenRedirection {
                        repo_path: "foo".into(),
                        target: None,
                    },
                ],
            }),
        };
        assert_eq!(
            Path::new("some/repo/file"),
            wv.resolve_redirection(Path::new("some/repo/file"))
        );
        assert_eq!(
            Path::new("/other/buck-out/redirected/file"),
            wv.resolve_redirection(Path::new("buck-out/redirected/file"),)
        );
        assert_eq!(
            Path::new("/other/buck-out/redirected/file"),
            wv.resolve_redirection(Path::new("/path/to/repo/buck-out/redirected/file"),)
        );
    }
}
