/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use antlir2_isolate::isolate;
use antlir2_isolate::IsolatedContext;
use antlir2_isolate::IsolationContext;
use image_test_lib::KvPair;
use once_cell::sync::OnceCell;
use thiserror::Error;
use tracing::debug;

use crate::utils::log_command;

#[derive(Error, Debug)]
pub(crate) enum IsolationError {
    #[error("Failed to find repo root: `{0}`")]
    RepoRootError(String),
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
        let repo = find_root::find_repo_root(
            &absolute_path::AbsolutePathBuf::canonicalize(
                env::current_exe().map_err(|e| IsolationError::RepoRootError(e.to_string()))?,
            )
            .map_err(|e| IsolationError::RepoRootError(e.to_string()))?,
        )
        .map_err(|e| IsolationError::RepoRootError(e.to_string()))?;
        Ok(PathBuf::from(repo))
    }

    /// Query the environment and set PLATFORM. Should be called exactly once
    /// before `get` is invoked.
    pub(crate) fn set() -> Result<()> {
        let repo = Platform::repo_root()?;
        let platform = Platform {
            paths: HashSet::from([
                repo,
                #[cfg(facebook)]
                PathBuf::from("/usr/local/fbcode"),
                #[cfg(facebook)]
                PathBuf::from("/mnt/gvfs"),
            ]),
        };

        PLATFORM
            .set(platform)
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

/// If these env exist, always pass them through.
const PASSTHROUGH_ENVS: &[&str] = &["RUST_LOG", "ANTLIR_BUCK"];

/// Generate default passthrough env vars
pub(crate) fn default_passthrough_envs() -> Vec<KvPair> {
    PASSTHROUGH_ENVS
        .iter()
        .filter(|x| env::var(*x).is_ok())
        .map(|x| KvPair {
            key: x.to_string(),
            value: OsString::from(env::var(x).expect("must exist")),
        })
        .collect()
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
        .register(true)
        .platform(Platform::get().clone())
        .working_directory(&repo)
        .outputs(outputs);
    builder.setenv(
        envs.into_iter()
            .map(|p| (p.key, p.value))
            .collect::<BTreeMap<_, _>>(),
    );
    Ok(isolate(builder.build())?)
}

/// Basic check if current environment is isolated
/// TODO: Linux specific
pub(crate) fn is_isolated() -> Result<bool> {
    let mut command = Command::new("systemd-detect-virt");
    let output = log_command(&mut command).output()?.stdout;
    let virt = std::str::from_utf8(&output)?.trim();
    debug!("systemd-detect-virt returned: {}", virt);
    Ok(virt == "systemd-nspawn")
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_default_envs() {
        assert_eq!(default_passthrough_envs(), Vec::new());
        env::set_var("RUST_LOG", "hello");
        assert_eq!(
            default_passthrough_envs(),
            vec![KvPair {
                key: "RUST_LOG".into(),
                value: "hello".into(),
            }],
        );
    }
}
