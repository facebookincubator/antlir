/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(io_error_more)]

use std::collections::HashMap;
use std::fmt::Display;
use std::fs::File;
use std::io::BufReader;
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use antlir2_features::Feature;
use buck_label::Label;
use cap_std::fs::Dir;
use nix::libc;
use openat2::openat2;
use openat2::OpenHow;
use openat2::ResolveFlags;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;

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
    #[error(transparent)]
    Feature(#[from] antlir2_features::Error),
    #[error(transparent)]
    Isolate(#[from] antlir2_isolate::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug)]
pub struct CompilerContext {
    /// Buck label of the image being built
    label: Label,
    /// Architecture of the image being built (may not be the same as the host
    /// architecture)
    target_arch: Arch,
    /// Path to the root of the image being built
    root_path: PathBuf,
    /// Open fd to the image root directory
    root: Dir,
    plans: HashMap<String, serde_json::Value>,
}

fn parse_file<T, E>(f: File) -> Result<T>
where
    T: FromStr<Err = E>,
    Error: From<E>,
{
    let src = std::io::read_to_string(BufReader::new(f))?;
    T::from_str(&src).map_err(Error::from)
}

impl CompilerContext {
    pub fn new(
        label: Label,
        target_arch: Arch,
        root: PathBuf,
        plans: HashMap<String, serde_json::Value>,
    ) -> Result<Self> {
        let root_fd = Dir::open_ambient_dir(&root, cap_std::ambient_authority())?;
        Ok(Self {
            label,
            target_arch,
            root_path: root,
            root: root_fd,
            plans,
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
        &self.root_path
    }

    pub fn plan<T>(&self, id: &str) -> Option<serde_json::Result<T>>
    where
        T: DeserializeOwned,
    {
        self.plans.get(id).cloned().map(serde_json::from_value)
    }

    /// Join a (possibly absolute) path with the root directory of the image
    /// being built.
    pub fn dst_path<P>(&self, path: P) -> std::io::Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        self.resolve_dst_path(path, ResolveMode::Parent)
    }

    fn resolve_dst_path<P>(&self, path: P, mode: ResolveMode) -> std::io::Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        if path.as_ref() == Path::new("/") || path.as_ref() == Path::new("") {
            return Ok(self.root_path.clone());
        }
        let mut how = OpenHow::new(libc::O_PATH, 0);
        how.resolve |= ResolveFlags::IN_ROOT;

        let resolve_path = match mode {
            ResolveMode::Full => path.as_ref(),
            ResolveMode::Parent => path.as_ref().parent().unwrap_or(path.as_ref()),
        };

        match openat2(Some(self.root.as_raw_fd()), resolve_path, &how) {
            Ok(fd) => {
                // TODO: this is gross, refactor this API into a more fd focused
                // interface which is good for 90% of use cases, and have only the 10%
                // that actually need paths use this mildly gross implementation
                let real_path = std::fs::read_link(format!("/proc/self/fd/{fd}"))?;
                nix::unistd::close(fd)?;
                Ok(match mode {
                    ResolveMode::Full => real_path,
                    ResolveMode::Parent => match path.as_ref().file_name() {
                        Some(n) => real_path.join(n),
                        None => real_path,
                    },
                })
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => {
                    // The path doesn't exist. OK, we can resolve the first path
                    // along the ancestor chain that does, then join the new
                    // parts that don't exist.
                    for ancestor in path
                        .as_ref()
                        .parent()
                        .expect("/ is always resolved")
                        .ancestors()
                    {
                        if let Ok(ancestor_real_path) =
                            self.resolve_dst_path(ancestor, ResolveMode::Full)
                        {
                            return Ok(ancestor_real_path.join(
                                path.as_ref()
                                    .strip_prefix(ancestor)
                                    .expect("ancestor can always be stripped"),
                            ));
                        }
                    }
                    unreachable!("the final ancestor / is always resolvable")
                }
                _ => Err(e),
            },
        }
    }

    pub fn user_db(&self) -> Result<antlir2_users::passwd::EtcPasswd> {
        match self.root.open("etc/passwd") {
            Ok(f) => parse_file(f.into_std()),
            Err(e) => match e.kind() {
                ErrorKind::NotFound => Ok(Default::default()),
                _ => Err(e.into()),
            },
        }
    }

    pub fn shadow_db(&self) -> Result<antlir2_users::shadow::EtcShadow> {
        match self.root.open("etc/shadow") {
            Ok(f) => parse_file(f.into_std()),
            Err(e) => match e.kind() {
                ErrorKind::NotFound => Ok(Default::default()),
                _ => Err(e.into()),
            },
        }
    }

    pub fn groups_db(&self) -> Result<antlir2_users::group::EtcGroup> {
        match self.root.open("etc/group") {
            Ok(f) => parse_file(f.into_std()),
            Err(e) => match e.kind() {
                ErrorKind::NotFound => Ok(Default::default()),
                _ => Err(e.into()),
            },
        }
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ResolveMode {
    /// Resolve the entirety of the path
    Full,
    /// Resolve symlinks down to the parent but leave the last component alone
    Parent,
}

pub trait CompileFeature {
    fn compile(&self, ctx: &CompilerContext) -> Result<()>;
}

static_assertions::assert_obj_safe!(CompileFeature);

/// PluginExt indirects the implementation of [CompileFeature] through a .so
/// plugin. The underlying crates all provide a type that implements
/// [CompileFeature], and some generated code provides a set of exported symbols
/// that let us call that implementation.
trait PluginExt {
    fn as_compile_feature_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature) -> antlir2_features::Result<Box<dyn CompileFeature>>>>;
}

impl PluginExt for antlir2_features::Plugin {
    fn as_compile_feature_fn(
        &self,
    ) -> Result<libloading::Symbol<fn(&Feature) -> antlir2_features::Result<Box<dyn CompileFeature>>>>
    {
        self.get_symbol(b"as_compile_feature\0")
            .map_err(antlir2_features::Error::from)
            .map_err(Error::from)
    }
}

impl CompileFeature for Feature {
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let func = self.plugin()?.as_compile_feature_fn()?;
        let feat = func(self)?;
        feat.compile(ctx)
    }
}
