/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! antlir2_isolate_compiler
//! ========================
//!
//! This crate serves to set up an isolated environment in which to perform
//! image compilation. This does not do any of the compilation or deal with
//! subvolume management, it simply prepares an isolation environment with
//! already-existing images.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

mod sys;

/// Everything needed to know how to isolate an image compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolationContext<'a> {
    /// A pre-built image (or the host rootfs when bootstrapping a completely
    /// new one from scratch) that contains tools necessary for building images.
    ///
    /// On Linux, it must be a pre-mounted directory that contains a full OS
    /// tree that `systemd-nspawn(1)` will accept.
    pub build_appliance: &'a Path,
    /// Set of paths to share into the isolated environment so that the
    /// compilation process has files that it depends on (for example, .so
    /// libraries linked into the compiler). This also should include the
    /// compiler itself!
    pub compiler_platform: BTreeSet<&'a Path>,
    /// Directory in which to invoke the provided command.
    pub working_directory: Option<&'a Path>,
    /// Set environment variables within the isolated environment.
    pub setenv: BTreeMap<&'a str, Cow<'a, OsStr>>,
    /// Set of paths to share into the isolated environment that the compilation
    /// will need to do the actual image build (for example, directories with
    /// files that are being copied into the image).
    pub image_sources: BTreeSet<&'a Path>,
    /// Root directory (as seen from the host) of the image being built.
    pub root: &'a Path,
    /// Set of paths that should be writable from within the isolated
    /// environment (for functions that require writing output files).
    pub writable_outputs: BTreeSet<&'a Path>,
}

impl Default for IsolationContext<'static> {
    fn default() -> Self {
        Self {
            build_appliance: Path::new("/"),
            compiler_platform: Default::default(),
            working_directory: None,
            setenv: Default::default(),
            image_sources: Default::default(),
            root: Path::new("/tmp/out"),
            writable_outputs: Default::default(),
        }
    }
}

/// Dynamic information about the isolated environment that might be necessary
/// for the image build.
#[derive(Debug)]
pub struct IsolatedCompilerContext {
    /// Root directory (as seen from the isolated compiler environment) of the
    /// image being built.
    pub root: PathBuf,
    /// Isolation command to which the compiler path and args should be
    /// appended.
    pub command: Command,
}

/// Set up an isolated environment to run a compilation process.
pub use sys::isolate_compiler;
