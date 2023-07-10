/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::path::Path;

use antlir2_features::genrule::Genrule;
use antlir2_isolate::isolate;
use antlir2_isolate::InvocationType;
use antlir2_isolate::IsolationContext;
use anyhow::anyhow;

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
        let mut cmd = isolate(
            IsolationContext::builder(ctx.root())
                .user(self.user.name())
                .ephemeral(false)
                .platform([
                    cwd.as_path(),
                    #[cfg(facebook)]
                    Path::new("/usr/local/fbcode"),
                    #[cfg(facebook)]
                    Path::new("/mnt/gvfs"),
                ])
                .working_directory(&cwd)
                .invocation_type(match self.boot {
                    true => InvocationType::BootReadOnly,
                    false => InvocationType::Pid2Pipe,
                })
                .build(),
        )
        .into_command();
        cmd.args(
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
        Ok(())
    }
}
