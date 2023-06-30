/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use antlir2_features::genrule::Genrule;
use anyhow::anyhow;
use anyhow::Context;
use itertools::Itertools;

use crate::CompileFeature;
use crate::CompilerContext;
use crate::Error;
use crate::Result;

impl<'a> CompileFeature for Genrule<'a> {
    #[tracing::instrument(name = "genrule", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        if self.boot {
            unimplemented!("boot is not yet implemented");
        }
        if self.bind_repo_ro {
            unimplemented!("bind_repo_ro is not yet implemented");
        }
        let cwd = std::env::current_dir()?;
        let mut cmd = Command::new("/__antlir2__/bwrap");
        cmd.arg("--bind")
            .arg(ctx.root())
            .arg("/")
            .args(["--proc", "/proc"]);

        let mut support_binds = vec![cwd.as_path()];
        #[cfg(facebook)]
        support_binds.extend(vec![Path::new("/usr/local/fbcode"), Path::new("/mnt/gvfs")]);

        cmd.args(
            support_binds
                .iter()
                .flat_map(|path| vec![OsStr::new("--ro-bind"), path.as_os_str(), path.as_os_str()]),
        );

        // record any directories created solely as supporting bind-mounts so
        // they can be deleted
        let support_dirs_to_clean_up: Vec<_> = support_binds
            .iter()
            .flat_map(|path| path.ancestors())
            .filter(|path| !ctx.dst_path(path).exists())
            .sorted_by_key(|path| path.components().count())
            .collect();

        cmd.arg("--").args(
            self.cmd
                .iter()
                .map(|s| OsStr::new(s.as_ref()))
                .collect::<Vec<_>>(),
        );
        tracing::trace!("executing genrule with isolated command: {cmd:?}");
        let res = cmd.output().map_err(Error::IO)?;
        if !res.status.success() {
            return Err(Error::Other(anyhow!(
                "genrule {self:?} {}. {}\n{}",
                match res.status.code() {
                    Some(code) => format!("exited with code {code}"),
                    None => "was terminated by a signal".to_owned(),
                },
                std::str::from_utf8(&res.stdout).unwrap_or("<invalid utf8>"),
                std::str::from_utf8(&res.stderr).unwrap_or("<invalid utf8>"),
            )));
        }

        for dir in support_dirs_to_clean_up.iter().rev() {
            std::fs::remove_dir(ctx.dst_path(dir))
                .with_context(|| format!("while deleting bind-mount dst '{}'", dir.display()))?;
        }

        Ok(())
    }
}
