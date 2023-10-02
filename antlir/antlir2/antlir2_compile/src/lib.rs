/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(file_set_times)]
#![feature(io_error_more)]
#![feature(io_error_other)]
#![feature(unix_chown)]

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Display;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use antlir2_features::Feature;
use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

#[cfg(facebook)]
pub mod facebook;

pub mod plan;
pub mod util;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no such user '{0}' in image")]
    NoSuchUser(String),
    #[error("no such group '{0}' in image")]
    NoSuchGroup(String),
    #[error(transparent)]
    LoadUsers(#[from] antlir2_users::Error),
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error("extract has conflict: want to install a different version of {0:?}")]
    ExtractConflict(PathBuf),
    #[error("install src {src:?} is a directory, but {dst:?} is missing trailing /")]
    InstallSrcIsDirectoryButNotDst { src: PathBuf, dst: PathBuf },
    #[error("install dst {dst:?} is claiming to be a directory, but {src:?} is a file")]
    InstallDstIsDirectoryButNotSrc { src: PathBuf, dst: PathBuf },
    #[error(transparent)]
    Feature(#[from] antlir2_features::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    Aarch64,
    X86_64,
}

impl FromStr for Arch {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "aarch64" => Ok(Self::Aarch64),
            "x86_64" => Ok(Self::X86_64),
            _ => Err(s.to_owned()),
        }
    }
}

impl Display for Arch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Aarch64 => "aarch64",
            Self::X86_64 => "x86_64",
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'static"))]
pub struct CompilerContext {
    /// Buck label of the image being built
    label: Label,
    /// Architecture of the image being built (may not be the same as the host
    /// architecture)
    target_arch: Arch,
    /// Path to the root of the image being built
    root: PathBuf,
    /// Setup information for dnf repos
    dnf: DnfContext,
    /// Pre-computed plan for this compilation phase
    plan: Option<plan::Plan>,
    #[cfg(facebook)]
    /// Resolved fbpkgs from plan
    fbpkg: facebook::FbpkgContext,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DnfContext {
    /// Root directory where dnf repos are mounted
    repos: PathBuf,
    /// Versionlock of package name -> EVRA
    versionlock: BTreeMap<String, String>,
    /// Rpms to exclude from all operations
    excluded_rpms: BTreeSet<String>,
}

impl DnfContext {
    pub fn new(
        repos: PathBuf,
        versionlock: BTreeMap<String, String>,
        excluded_rpms: BTreeSet<String>,
    ) -> Self {
        Self {
            repos,
            versionlock,
            excluded_rpms,
        }
    }

    pub fn repos(&self) -> &Path {
        &self.repos
    }

    pub fn versionlock(&self) -> &BTreeMap<String, String> {
        &self.versionlock
    }

    pub fn excluded_rpms(&self) -> &BTreeSet<String> {
        &self.excluded_rpms
    }
}

fn parse_file<T, E>(path: &Path) -> Option<Result<T>>
where
    T: FromStr<Err = E>,
    Error: From<E>,
{
    match std::fs::read_to_string(path) {
        Ok(src) => Some(T::from_str(&src).map_err(Error::from)),
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => None,
            _ => Some(Err(e.into())),
        },
    }
}

impl CompilerContext {
    pub fn new(
        label: Label,
        target_arch: Arch,
        root: PathBuf,
        dnf: DnfContext,
        plan: Option<plan::Plan>,
        #[cfg(facebook)] fbpkg: facebook::FbpkgContext,
    ) -> Result<Self> {
        Ok(Self {
            label,
            target_arch,
            root,
            dnf,
            plan,
            #[cfg(facebook)]
            fbpkg,
        })
    }

    pub fn label(&self) -> &Label {
        &self.label
    }

    pub fn target_arch(&self) -> Arch {
        self.target_arch
    }

    /// Root directory for the image being built
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dnf(&self) -> &DnfContext {
        &self.dnf
    }

    #[cfg(facebook)]
    pub fn fbpkg(&self) -> &facebook::FbpkgContext {
        &self.fbpkg
    }

    pub fn plan(&self) -> Option<&plan::Plan> {
        self.plan.as_ref()
    }

    /// Join a (possibly absolute) path with the root directory of the image
    /// being built.
    pub fn dst_path<P>(&self, path: P) -> PathBuf
    where
        P: AsRef<Path>,
    {
        if !path.as_ref().is_absolute() {
            self.root.join(path)
        } else if path.as_ref().starts_with(&self.root) {
            path.as_ref().to_path_buf()
        } else {
            self.root
                .join(path.as_ref().strip_prefix("/").expect("infallible"))
        }
    }

    pub fn user_db(&self) -> Result<antlir2_users::passwd::EtcPasswd> {
        parse_file(&self.dst_path("/etc/passwd")).unwrap_or_else(|| Ok(Default::default()))
    }

    pub fn shadow_db(&self) -> Result<antlir2_users::shadow::EtcShadow> {
        parse_file(&self.dst_path("/etc/shadow")).unwrap_or_else(|| Ok(Default::default()))
    }

    pub fn groups_db(&self) -> Result<antlir2_users::group::EtcGroup> {
        parse_file(&self.dst_path("/etc/group")).unwrap_or_else(|| Ok(Default::default()))
    }

    /// Get the uid for a user inside of the image being built
    pub fn uid(&self, name: &str) -> Result<antlir2_users::UserId> {
        self.user_db()?
            .get_user_by_name(name)
            .map(|u| u.uid)
            .ok_or_else(|| Error::NoSuchUser(name.to_owned()))
    }

    /// Get the gid for a group inside of the image being built
    pub fn gid(&self, name: &str) -> Result<antlir2_users::GroupId> {
        self.groups_db()?
            .get_group_by_name(name)
            .map(|g| g.gid)
            .ok_or_else(|| Error::NoSuchGroup(name.to_owned()))
    }
}

pub trait CompileFeature {
    fn compile(&self, ctx: &CompilerContext) -> Result<()>;

    /// Add details about this [Feature] to the compiler [plan::Plan].
    fn plan(&self, _ctx: &CompilerContext) -> Result<Vec<plan::Item>> {
        Ok(Default::default())
    }
}

/// PluginExt indirects the implementation of [CompileFeature] through a .so
/// plugin. The underlying crates all provide a type that implements
/// [CompileFeature], and some generated code provides a set of exported symbols
/// that let us call that implementation.
trait PluginExt {
    fn compile_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&antlir2_features::Feature, &CompilerContext) -> Result<()>>>;
    fn plan_fn(
        &self,
    ) -> Result<
        libloading::Symbol<
            fn(&antlir2_features::Feature, &CompilerContext) -> Result<Vec<plan::Item>>,
        >,
    >;
}

impl PluginExt for antlir2_features::Plugin {
    fn compile_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature, &CompilerContext) -> Result<()>>> {
        self.get_symbol(b"CompileFeature_compile\0")
            .map_err(|e| anyhow::anyhow!("while getting compile function: {e}").into())
    }

    fn plan_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature, &CompilerContext) -> Result<Vec<plan::Item>>>> {
        self.get_symbol(b"CompileFeature_plan\0")
            .map_err(|e| anyhow::anyhow!("while getting plan function: {e}").into())
    }
}

impl CompileFeature for Feature {
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        self.plugin()?.compile_fn()?(self, ctx)
    }

    fn plan(&self, ctx: &CompilerContext) -> Result<Vec<plan::Item>> {
        self.plugin()?.plan_fn()?(self, ctx)
    }
}
