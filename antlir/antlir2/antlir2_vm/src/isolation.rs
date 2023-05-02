/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_isolate::isolate;
use antlir2_isolate::IsolatedContext;
use antlir2_isolate::IsolationContext;
use once_cell::sync::OnceCell;
use thiserror::Error;
use tracing::debug;

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

/// If these env exist, pass them into the container too.
const PASSTHROUGH_ENVS: &[&str] = &["RUST_LOG", "ANTLIR_BUCK"];

/// Concatenate PASSTHROUGH_ENVS and `envs` to create a list of env names that
/// should be passed through
fn env_filter(envs: Option<&[String]>) -> Vec<&str> {
    match envs {
        Some(envs) => envs
            .iter()
            .map(|x| x.as_str())
            .chain(PASSTHROUGH_ENVS.iter().copied())
            .collect(),
        None => PASSTHROUGH_ENVS.to_vec(),
    }
}

/// Return IsolatedContext ready for executing a command inside isolation
/// # Arguments
/// * `image` - container image that would be used to run the VM
/// * `envs` - Additional envs to set inside container.
pub(crate) fn isolated(image: PathBuf, envs: Option<&[String]>) -> Result<IsolatedContext> {
    let mut builder = IsolationContext::builder(image);
    builder
        .register(true)
        .platform(Platform::get().clone())
        .outputs([
            // Carry over virtualizations support
            // TODO: Linux-specific
            Path::new("/dev/kvm"),
        ]);
    let filter = env_filter(envs);
    env::vars()
        .filter(|(k, _)| filter.contains(&k.as_str()))
        .for_each(|(k, v)| {
            builder.setenv::<(String, OsString)>((k, v.into()));
        });

    Ok(isolate(builder.build()))
}

/// Basic check if current environment is isolated
/// TODO: Linux specific
pub(crate) fn is_isolated() -> Result<bool> {
    let output = Command::new("systemd-detect-virt").output()?.stdout;
    let virt = std::str::from_utf8(&output)?.trim();
    debug!("systemd-detect-virt returned: {}", virt);
    Ok(virt == "systemd-nspawn")
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_env_filter() {
        let all_envs = [("HELLO", "a"), ("WORLD", "b"), ("RUST_LOG", "c")];

        let filter = env_filter(None);
        assert_eq!(
            all_envs
                .into_iter()
                .filter(|(k, _)| filter.contains(k))
                .collect::<Vec<_>>(),
            vec![("RUST_LOG", "c")],
        );

        let envs = ["HELLO".to_string()];
        let filter = env_filter(Some(&envs));
        assert_eq!(
            all_envs
                .into_iter()
                .filter(|(k, _)| filter.contains(k))
                .collect::<Vec<_>>(),
            vec![("HELLO", "a"), ("RUST_LOG", "c")],
        );
    }
}
