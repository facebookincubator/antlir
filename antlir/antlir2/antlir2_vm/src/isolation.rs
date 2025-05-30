/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::IsolatedContext;
use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use image_test_lib::KvPair;
use once_cell::sync::OnceCell;
use thiserror::Error;
use tracing::warn;

use crate::types::MountPlatformDecision;

#[derive(Error, Debug)]
pub(crate) enum IsolationError {
    #[error("Failed to set platform")]
    PlatformError,
    #[error(transparent)]
    CommandError(#[from] std::io::Error),
    #[error(transparent)]
    FromUtf8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    Antlir2Isolate(#[from] antlir2_isolate::Error),
}
type Result<T> = std::result::Result<T, IsolationError>;

/// Platform paths that are shared into the container. They also need
/// to be shared inside VM.
pub(crate) struct Platform {
    paths: HashSet<PathBuf>,
}

/// Platform should be same once set. Enforce through OnceCell.
static PLATFORM: OnceCell<Platform> = OnceCell::new();

impl Platform {
    /// Get repo root
    pub(crate) fn repo_root() -> Result<PathBuf> {
        match find_root::find_repo_root(std::env::current_exe()?) {
            Ok(repo) => Ok(repo),
            Err(e) => {
                warn!("couldn't find repo root, just using cwd instead: {e:#?}");
                Ok(std::env::current_dir()?)
            }
        }
    }

    /// Query the environment and set PLATFORM. Should be called exactly once
    /// before `get` is invoked.
    pub(crate) fn set(mount_platform: &MountPlatformDecision) -> Result<()> {
        let repo = Platform::repo_root()?;

        let mut paths = HashSet::from([
            repo,
            #[cfg(facebook)]
            PathBuf::from("/mnt/gvfs"),
        ]);
        if cfg!(facebook) && mount_platform.0 {
            paths.insert(PathBuf::from("/usr/local/fbcode"));
        }

        PLATFORM
            .set(Platform { paths })
            .map_err(|_| IsolationError::PlatformError)
    }

    /// Return the populated platform. Should only be called after `set`.
    pub(crate) fn get() -> &'static HashSet<PathBuf> {
        &PLATFORM
            .get()
            .expect("get_platform called before initialization")
            .paths
    }
}

/// Return IsolatedContext ready for executing a command inside isolation
/// # Arguments
/// * `image` - container image that would be used to run the VM
/// * `envs` - env vars to set inside container.
/// * `outputs` - Additional writable directories
pub(crate) fn isolated(
    image: &PathBuf,
    envs: Vec<KvPair>,
    outputs: HashSet<PathBuf>,
) -> Result<IsolatedContext> {
    let repo = Platform::repo_root()?;
    let mut builder = IsolationContext::builder(image);
    builder
        .platform(Platform::get().clone())
        .working_directory(repo.clone())
        .tmpfs(Path::new("/run"))
        .devtmpfs(Path::new("/dev"))
        .tmpfs(Path::new("/mnt/xarfuse"))
        .outputs(outputs);
    builder.setenv(
        envs.into_iter()
            .map(|p| (p.key, p.value))
            .collect::<BTreeMap<_, _>>(),
    );
    Ok(unshare(builder.build())?)
}
