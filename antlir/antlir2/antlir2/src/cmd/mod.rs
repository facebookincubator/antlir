/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

use antlir2_compile::Arch;
use antlir2_compile::CompilerContext;
use antlir2_compile::DnfContext;
use antlir2_depgraph::Graph;
use buck_label::Label;
use clap::Parser;
use json_arg::Json;
use json_arg::JsonFile;

use crate::Error;
use crate::Result;

mod compile;
mod depgraph;
mod map;
mod plan;
mod shell;
#[cfg(facebook)]
use antlir2_compile::facebook::FbpkgContext;
pub(crate) use compile::Compile;
pub(crate) use depgraph::Depgraph;
pub(crate) use map::Map;
pub(crate) use plan::Plan;
pub(crate) use shell::Shell;

/// Args that are common to "compileish" commands (for now, 'compile' and
/// 'plan', but maybe others in the future)
#[derive(Parser, Debug)]
struct Compileish {
    #[clap(long)]
    /// Buck label of the image being built
    pub(crate) label: Label,
    #[clap(long)]
    /// Root directory of under-construction image. Must already exist (either
    /// empty or as a snapshot of a parent layer)
    pub(crate) root: PathBuf,
    #[clap(long)]
    /// Path to mounted build appliance image
    pub(crate) build_appliance: PathBuf,
    #[clap(flatten)]
    pub(crate) external: CompileishExternal,
    #[clap(flatten)]
    pub(crate) dnf: DnfCompileishArgs,
    #[cfg(facebook)]
    #[clap(flatten)]
    pub(crate) fbpkg: FbpkgCompileishArgs,
}

#[derive(Parser, Debug)]
struct DnfCompileishArgs {
    #[clap(long = "dnf-repos")]
    /// Path to available dnf repositories
    pub(crate) repos: PathBuf,
    #[clap(long = "dnf-versionlock")]
    /// Path to dnf versionlock json file
    pub(crate) versionlock: Option<JsonFile<BTreeMap<String, String>>>,
    #[clap(long = "dnf-versionlock-extend")]
    /// Pin RPM versions, overwrites `dnf-versionlock`
    pub(crate) versionlock_extend: Json<BTreeMap<String, String>>,
    #[clap(long = "dnf-excluded-rpms")]
    /// Path to json file with list of rpms to exclude from dnf operations
    pub(crate) excluded_rpms: Option<JsonFile<BTreeSet<String>>>,
}

#[cfg(facebook)]
#[derive(Parser, Debug)]
struct FbpkgCompileishArgs {
    #[clap(long = "resolved-fbpkgs")]
    /// Path to resolced fbpkgs json file
    pub(crate) resolved_fbpkgs:
        Option<JsonFile<BTreeMap<String, antlir2_compile::facebook::ResolvedFbpkgInfo>>>,
}

#[derive(Parser, Debug)]
/// Compile arguments that are _always_ passed from external sources (in other
/// words, by buck2 actions) and are never generated by internal code in the
/// 'isolate' subcommand.
struct CompileishExternal {
    #[clap(long)]
    /// Architecture of the image being built
    pub(crate) target_arch: Arch,
    #[clap(long = "depgraph-json")]
    /// Path to input depgraph json file with features to include in this image
    pub(crate) depgraph: JsonFile<Graph>,
}

impl Compileish {
    pub(super) fn compiler_context(
        &self,
        plan: Option<antlir2_compile::plan::Plan>,
    ) -> Result<CompilerContext> {
        let mut dnf_versionlock = self
            .dnf
            .versionlock
            .clone()
            .map(JsonFile::into_inner)
            .unwrap_or_default();
        dnf_versionlock.extend(self.dnf.versionlock_extend.clone().into_inner());
        CompilerContext::new(
            self.label.clone(),
            self.external.target_arch,
            self.root.clone(),
            self.build_appliance.clone(),
            DnfContext::new(
                self.dnf.repos.clone(),
                dnf_versionlock,
                self.dnf
                    .excluded_rpms
                    .as_ref()
                    .map(JsonFile::as_inner)
                    .cloned()
                    .unwrap_or_default(),
            ),
            plan,
            #[cfg(facebook)]
            FbpkgContext::new(self.fbpkg.resolved_fbpkgs.clone().map(JsonFile::into_inner)),
        )
        .map_err(Error::Compile)
    }
}
