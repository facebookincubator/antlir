/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(io_error_more)]

use std::collections::HashSet;
use std::fmt::Debug;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::Requirement;
use antlir2_features::types::UserName;
use antlir2_isolate::sys::unshare;
use antlir2_isolate::InvocationType;
use antlir2_isolate::IsolationContext;
use anyhow::Context;
use itertools::Itertools;
use serde::Deserialize;
use serde::Serialize;

pub type Feature = Genrule;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Genrule {
    pub cmd: Vec<ResolvedMacro>,
    pub user: UserName,
    pub bind_repo_ro: bool,
    pub mount_platform: bool,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ResolvedMacro(Vec<String>);

impl Debug for ResolvedMacro {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0[..] {
            [value] => write!(f, "{value:?}"),
            array => write!(f, "{:?}", array),
        }
    }
}

impl Genrule {
    fn cmd_iter(&self) -> impl Iterator<Item = &str> {
        self.cmd.iter().flat_map(|r| r.0.iter().map(String::as_str))
    }
}

impl antlir2_depgraph_if::RequiresProvides for Genrule {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(Default::default())
    }
}

impl antlir2_compile::CompileFeature for Genrule {
    #[tracing::instrument(name = "genrule", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let cwd = std::env::current_dir()?;
        let mut inner_cmd = self.cmd_iter();
        let mut isol = IsolationContext::builder(ctx.root_path());
        isol.user(&self.user)
            .ephemeral(false)
            .devtmpfs(Path::new("/dev"))
            .setenv(("TMPDIR", "/tmp"))
            .invocation_type(InvocationType::Pid2Pipe);
        if self.mount_platform {
            #[cfg(facebook)]
            isol.platform(["/usr/local/fbcode", "/mnt/gvfs"]);
        }
        if self.bind_repo_ro {
            isol.inputs(cwd.as_path()).working_directory(&cwd);
        } else {
            isol.working_directory(Path::new("/"));
        }
        let isol = isol.build();
        // find all the mountpoints (and their ancestors) that did not exist
        // before setting up the genrule environment, and remove them after the
        // genrule is done, asssuming they are all empty
        let mounts_to_cleanup: HashSet<_> = isol
            .inputs
            .keys()
            .chain(isol.platform.keys())
            .flat_map(|p| p.ancestors())
            .filter_map(|p| match ctx.dst_path(p) {
                Ok(real) => {
                    if !real.exists() {
                        Some(real)
                    } else {
                        None
                    }
                }
                Err(_) => None,
            })
            .collect();
        let mut cmd = unshare(isol)?.command(inner_cmd.next().expect("must have argv[0]"))?;
        cmd.args(inner_cmd);
        tracing::trace!("executing genrule with isolated command: {cmd:?}");
        let res = cmd.output().context("while running cmd")?;
        if !res.status.success() {
            return Err(anyhow::anyhow!(
                "genrule {self:?} {}. {}\n{}",
                match res.status.code() {
                    Some(code) => format!("exited with code {code}"),
                    None => "was terminated by a signal".to_owned(),
                },
                std::str::from_utf8(&res.stdout).unwrap_or("<invalid utf8>"),
                std::str::from_utf8(&res.stderr).unwrap_or("<invalid utf8>"),
            )
            .into());
        }
        // clean up any paths that existed solely for mountpoints of genrule
        // inputs, unless they are empty
        for path in mounts_to_cleanup
            .into_iter()
            .sorted_by_key(|p| p.components().count())
            .rev()
        {
            let meta = path.metadata()?;
            if meta.is_dir() {
                if let Err(e) = std::fs::remove_dir(&path) {
                    if e.kind() != ErrorKind::DirectoryNotEmpty {
                        return Err(anyhow::anyhow!(
                            "could not remove seemingly-unnecessary mountpoint '{}': {e:#?}",
                            path.display()
                        )
                        .into());
                    }
                }
            } else if meta.is_file() && meta.size() == 0 {
                std::fs::remove_file(&path)?;
            }
        }
        Ok(())
    }
}
