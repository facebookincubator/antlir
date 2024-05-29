/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Everything needed to know how to isolate an image compilation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsolationContext<'a> {
    pub layer: Cow<'a, Path>,
    /// See [IsolationContextBuilder::platform]
    pub platform: BTreeMap<Cow<'a, Path>, Cow<'a, Path>>,
    /// Directory in which to invoke the provided command.
    pub working_directory: Option<Cow<'a, Path>>,
    /// See [IsolationContextBuilder::setenv]
    pub setenv: BTreeMap<Cow<'a, OsStr>, Cow<'a, OsStr>>,
    /// See [IsolationContextBuilder::inputs]
    pub inputs: BTreeMap<Cow<'a, Path>, Cow<'a, Path>>,
    /// See [IsolationContextBuilder::outputs]
    pub outputs: BTreeMap<Cow<'a, Path>, Cow<'a, Path>>,
    /// See [InvocationType]
    pub invocation_type: InvocationType,
    /// See [IsolationContextBuilder::register]
    pub register: bool,
    /// See [IsolationContextBuilder::user]
    pub user: Cow<'a, str>,
    /// See [IsolationContextBuilder::ephemeral]
    pub ephemeral: bool,
    /// See [IsolationContextBuilder::tmpfs]
    pub tmpfs: BTreeSet<Cow<'a, Path>>,
    /// See [IsolationContextBuilder::devtmpfs]
    pub devtmpfs: BTreeSet<Cow<'a, Path>>,
    /// See [IsolationContextBuilder::tmpfs_overlay]
    pub tmpfs_overlay: BTreeSet<Cow<'a, Path>>,
    /// See [IsolationContextBuilder::hostname]
    pub hostname: Option<Cow<'a, str>>,
    /// See [IsolationContextBuilder::readonly]
    pub readonly: bool,
}

/// Controls how the container is spawned and how console is configured for the
/// container payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvocationType {
    /// Runs /init from inside the layer as PID 1, sets console as read-only
    BootReadOnly,
    /// Invokes command in layer as PID 2 w/ stub PID 1, sets console as interactive
    Pid2Interactive,
    /// Invokes command in layer as PID 2 w/ stub PID 1, sets console as pipe (helpful
    /// when using in pipelines)
    Pid2Pipe,
}

impl InvocationType {
    pub fn booted(&self) -> bool {
        self == &InvocationType::BootReadOnly
    }
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
                invocation_type: InvocationType::Pid2Pipe,
                register: false,
                user: Cow::Borrowed("root"),
                ephemeral: true,
                tmpfs: Default::default(),
                devtmpfs: Default::default(),
                tmpfs_overlay: Default::default(),
                hostname: None,
                readonly: false,
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

    /// See [InvocationType]
    pub fn invocation_type<I: Into<InvocationType>>(&mut self, invocation_type: I) -> &mut Self {
        self.ctx.invocation_type = invocation_type.into();
        self
    }

    /// Register the isolated environment with a random name. The name can be
    /// printed out from the provided command for debugging purpose.
    pub fn register(&mut self, register: bool) -> &mut Self {
        self.ctx.register = register;
        self
    }

    /// Run the isolated command as a specific user.
    pub fn user<S: Into<Cow<'a, str>>>(&mut self, user: S) -> &mut Self {
        self.ctx.user = user.into();
        self
    }

    /// Set up the isolated environment to be thrown away after running. When
    /// false, the root layer will be mutable.
    pub fn ephemeral(&mut self, ephemeral: bool) -> &mut Self {
        self.ctx.ephemeral = ephemeral;
        self
    }

    /// Path to mount a (unique) tmpfs into.
    pub fn tmpfs<P: Into<Cow<'a, Path>>>(&mut self, path: P) -> &mut Self {
        self.ctx.tmpfs.insert(path.into());
        self
    }

    /// Path to mount a devtmpfs into.
    pub fn devtmpfs<P: Into<Cow<'a, Path>>>(&mut self, path: P) -> &mut Self {
        self.ctx.devtmpfs.insert(path.into());
        self
    }

    /// Mount a unique tmpfs as the top layer of an overlayfs over this
    /// directory (in other words, make this directory read/write with ephemeral
    /// changes).
    pub fn tmpfs_overlay<P: Into<Cow<'a, Path>>>(&mut self, path: P) -> &mut Self {
        self.ctx.tmpfs_overlay.insert(path.into());
        self
    }

    /// Set the hostname in the container
    pub fn hostname<S: Into<Cow<'a, str>>>(&mut self, hostname: S) -> &mut Self {
        self.ctx.hostname = Some(hostname.into());
        self
    }

    /// Start the container with a readonly root.
    pub fn readonly(&mut self) -> &mut Self {
        self.ctx.readonly = true;
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

impl<'a> IntoBinds<'a> for &'a str {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        HashMap::from([(
            Cow::Borrowed(Path::new(self)),
            Cow::Borrowed(Path::new(self)),
        )])
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

impl<'a> IntoBinds<'a> for (&'a Path, PathBuf) {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        HashMap::from([(Cow::Borrowed(self.0), Cow::Owned(self.1))])
    }
}

impl<'a> IntoBinds<'a> for (&'a str, &'a str) {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        HashMap::from([(
            Cow::Borrowed(Path::new(self.0)),
            Cow::Borrowed(Path::new(self.1)),
        )])
    }
}

impl<'a> IntoBinds<'a> for (&'a str, &'a Path) {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        HashMap::from([(Cow::Borrowed(Path::new(self.0)), Cow::Borrowed(self.1))])
    }
}

impl<'a> IntoBinds<'a> for (PathBuf, PathBuf) {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        HashMap::from([(Cow::Owned(self.0), Cow::Owned(self.1))])
    }
}

impl<'a> IntoBinds<'a> for &[&'a Path] {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.iter()
            .map(|path| (Cow::Borrowed(*path), Cow::Borrowed(*path)))
            .collect()
    }
}

impl<'a> IntoBinds<'a> for &[&'a str] {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.iter()
            .map(|path| {
                (
                    Cow::Borrowed(Path::new(*path)),
                    Cow::Borrowed(Path::new(*path)),
                )
            })
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

impl<'a, const N: usize> IntoBinds<'a> for [&'a str; N] {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.into_iter()
            .map(|path| {
                (
                    Cow::Borrowed(Path::new(path)),
                    Cow::Borrowed(Path::new(path)),
                )
            })
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

impl<'a> IntoBinds<'a> for HashSet<&'a Path> {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.into_iter()
            .map(|path| (Cow::Borrowed(path), Cow::Borrowed(path)))
            .collect()
    }
}

impl<'a> IntoBinds<'a> for HashSet<PathBuf> {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.into_iter()
            .map(|path| (Cow::Owned(path.clone()), Cow::Owned(path)))
            .collect()
    }
}

impl<'a> IntoBinds<'a> for HashMap<PathBuf, PathBuf> {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.into_iter()
            .map(|(dst, src)| (Cow::Owned(dst), Cow::Owned(src)))
            .collect()
    }
}

impl<'a> IntoBinds<'a> for HashMap<&'a Path, &'a Path> {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.into_iter()
            .map(|(dst, src)| (Cow::Borrowed(dst), Cow::Borrowed(src)))
            .collect()
    }
}

impl<'a> IntoBinds<'a> for HashMap<&'a str, &'a str> {
    fn into_binds(self) -> HashMap<Cow<'a, Path>, Cow<'a, Path>> {
        self.into_iter()
            .map(|(dst, src)| (Cow::Borrowed(Path::new(dst)), Cow::Borrowed(Path::new(src))))
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

impl<'a> IntoEnv<'a> for (&'a str, &'a str) {
    fn into_env(self) -> HashMap<Cow<'a, OsStr>, Cow<'a, OsStr>> {
        HashMap::from([(
            Cow::Borrowed(OsStr::new(self.0)),
            Cow::Borrowed(OsStr::new(self.1)),
        )])
    }
}

impl<'a> IntoEnv<'a> for (&'a str, &'a Path) {
    fn into_env(self) -> HashMap<Cow<'a, OsStr>, Cow<'a, OsStr>> {
        HashMap::from([(
            Cow::Borrowed(OsStr::new(self.0)),
            Cow::Borrowed(OsStr::new(self.1.as_os_str())),
        )])
    }
}

impl<'a> IntoEnv<'a> for (&'a str, OsString) {
    fn into_env(self) -> HashMap<Cow<'a, OsStr>, Cow<'a, OsStr>> {
        HashMap::from([(Cow::Borrowed(OsStr::new(self.0)), Cow::Owned(self.1))])
    }
}

impl<'a> IntoEnv<'a> for (String, OsString) {
    fn into_env(self) -> HashMap<Cow<'a, OsStr>, Cow<'a, OsStr>> {
        HashMap::from([(Cow::Owned(OsString::from(self.0)), Cow::Owned(self.1))])
    }
}

impl<'a> IntoEnv<'a> for BTreeMap<String, OsString> {
    fn into_env(self) -> HashMap<Cow<'a, OsStr>, Cow<'a, OsStr>> {
        self.into_iter()
            .map(|(k, v)| (OsString::from(k).into(), v.into()))
            .collect()
    }
}

impl<'a> IntoEnv<'a> for BTreeMap<String, String> {
    fn into_env(self) -> HashMap<Cow<'a, OsStr>, Cow<'a, OsStr>> {
        self.into_iter()
            .map(|(k, v)| (OsString::from(k).into(), OsString::from(v).into()))
            .collect()
    }
}

impl<'a> IntoEnv<'a> for (String, PathBuf) {
    fn into_env(self) -> HashMap<Cow<'a, OsStr>, Cow<'a, OsStr>> {
        HashMap::from([(
            Cow::Owned(OsString::from(self.0)),
            Cow::Owned(self.1.into_os_string()),
        )])
    }
}
