/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! antlir2_isolate
//! ===============
//!
//! This crate serves to set up an isolated environment in which to perform
//! image compilation. This does not do any of the compilation or deal with
//! subvolume management, it simply prepares an isolation environment with
//! already-existing images.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

mod sys;

/// Everything needed to know how to isolate an image compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolationContext<'a> {
    layer: Cow<'a, Path>,
    /// See [IsolationContextBuilder::platform]
    platform: BTreeMap<Cow<'a, Path>, Cow<'a, Path>>,
    /// Directory in which to invoke the provided command.
    working_directory: Option<Cow<'a, Path>>,
    /// See [IsolationContextBuilder::setenv]
    setenv: BTreeMap<Cow<'a, OsStr>, Cow<'a, OsStr>>,
    /// See [IsolationContextBuilder::inputs]
    inputs: BTreeMap<Cow<'a, Path>, Cow<'a, Path>>,
    /// See [IsolationContextBuilder::outputs]
    outputs: BTreeMap<Cow<'a, Path>, Cow<'a, Path>>,
}

impl<'a> IsolationContext<'a> {
    /// Start making an [IsolationContext] with a pre-built image (or the host
    /// rootfs when bootstrapping a completely new one from scratch) that
    /// contains tools necessary to run the isolated binary.
    ///
    /// On Linux, it must be a pre-mounted directory that contains a full OS
    /// tree that `systemd-nspawn(1)` will accept.
    pub fn builder<P: Into<Cow<'a, Path>>>(layer: P) -> IsolationContextBuilder<'a> {
        IsolationContextBuilder {
            ctx: Self {
                layer: layer.into(),
                platform: Default::default(),
                working_directory: None,
                setenv: Default::default(),
                inputs: Default::default(),
                outputs: Default::default(),
            },
        }
    }

    /// Build an [IsolationContext] using _only_ a layer. This is almost never useful
    pub fn new<P: Into<Cow<'a, Path>>>(layer: P) -> Self {
        Self::builder(layer).build()
    }
}

/// Build an [IsolationContext] more easily than passing every struct field
#[derive(Debug, Clone)]
pub struct IsolationContextBuilder<'a> {
    ctx: IsolationContext<'a>,
}

impl<'a> IsolationContextBuilder<'a> {
    /// Set of paths to share into the isolated environment so that the isolated
    /// binary has files that it depends on (for example, .so libraries linked
    /// into the binary). This also should include the isolated binary itself!
    pub fn platform<P: IntoBinds<'a>>(&mut self, paths: P) -> &mut Self {
        self.ctx.platform.extend(paths.into_binds());
        self
    }

    /// Set of paths to share into the isolated environment that the compilation
    /// will need to do the actual image build (for example, directories with
    /// files that are being copied into the image).
    pub fn inputs<P: IntoBinds<'a>>(&mut self, paths: P) -> &mut Self {
        self.ctx.inputs.extend(paths.into_binds());
        self
    }

    /// Set of paths that should be writable from within the isolated
    /// environment (for functions that require writing output files).
    pub fn outputs<P: IntoBinds<'a>>(&mut self, paths: P) -> &mut Self {
        self.ctx.outputs.extend(paths.into_binds());
        self
    }

    /// Set environment variables within the isolated environment.
    pub fn setenv<E: IntoEnv<'a>>(&mut self, env: E) -> &mut Self {
        self.ctx.setenv.extend(env.into_env());
        self
    }

    /// Directory in which to invoke the provided command.
    pub fn working_directory<P: Into<Cow<'a, Path>>>(&mut self, path: P) -> &mut Self {
        self.ctx.working_directory = Some(path.into());
        self
    }

    /// Finalize the IsolationContext
    pub fn build(&mut self) -> IsolationContext<'a> {
        self.ctx.clone()
    }
}

/// Anything that can be turned into a set of bind mounts
pub trait IntoBinds<'a> {
    /// Map dst->src
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>>;
}

impl<'a> IntoBinds<'a> for &'a Path {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        HashMap::from([(Cow::Borrowed(self), Cow::Borrowed(self))])
    }
}

impl<'a> IntoBinds<'a> for PathBuf {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        HashMap::from([(Cow::Owned(self.clone()), Cow::Owned(self))])
    }
}

impl<'a> IntoBinds<'a> for (&'a Path, &'a Path) {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        HashMap::from([(Cow::Borrowed(self.0), Cow::Borrowed(self.1))])
    }
}

impl<'a> IntoBinds<'a> for &[&'a Path] {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.iter()
            .map(|path| (Cow::Borrowed(*path), Cow::Borrowed(*path)))
            .collect()
    }
}

impl<'a, const N: usize> IntoBinds<'a> for [&'a Path; N] {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.into_iter()
            .map(|path| (Cow::Borrowed(path), Cow::Borrowed(path)))
            .collect()
    }
}

impl<'a> IntoBinds<'a> for BTreeSet<&'a Path> {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.into_iter()
            .map(|path| (Cow::Borrowed(path), Cow::Borrowed(path)))
            .collect()
    }
}

/// Anything that can be turned into a set of env variables
pub trait IntoEnv<'a> {
    fn into_env(self) -> HashMap<Cow<'a, OsStr>, Cow<'a, OsStr>>;
}

impl<'a> IntoEnv<'a> for (&'a OsStr, &'a OsStr) {
    fn into_env(self) -> HashMap<Cow<'a, OsStr>, Cow<'a, OsStr>> {
        HashMap::from([(Cow::Borrowed(self.0), Cow::Borrowed(self.1))])
    }
}

impl<'a> IntoEnv<'a> for (&'a str, OsString) {
    fn into_env(self) -> HashMap<Cow<'a, OsStr>, Cow<'a, OsStr>> {
        HashMap::from([(Cow::Borrowed(OsStr::new(self.0)), Cow::Owned(self.1))])
    }
}

/// Dynamic information about the isolated environment that might be necessary
/// for the image build.
#[derive(Debug)]
pub struct IsolatedContext {
    /// Isolation command to which the compiler path and args should be
    /// appended.
    pub command: Command,
}

/// Set up an isolated environment to run a compilation process.
pub use sys::isolate;
