/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::Path;

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_features::types::UserName;
use antlir2_isolate::isolate;
use antlir2_isolate::InvocationType;
use antlir2_isolate::IsolationContext;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use derivative::Derivative;
use serde::Deserialize;
use serde::Serialize;

pub type Feature = Genrule<'static>;

#[derive(Debug, Clone, PartialEq, Eq, Derivative, Deserialize, Serialize)]
#[derivative(PartialOrd, Ord)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Genrule<'a> {
    pub cmd: Vec<Cow<'a, str>>,
    pub user: UserName<'a>,
    pub boot: bool,
    pub bind_repo_ro: bool,
}

impl<'f> antlir2_feature_impl::Feature<'f> for Genrule<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement<'f>>> {
        Ok(Default::default())
    }

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
        let res = cmd.output().context("while running cmd")?;
        ensure!(
            res.status.success(),
            "genrule {self:?} {}. {}\n{}",
            match res.status.code() {
                Some(code) => format!("exited with code {code}"),
                None => "was terminated by a signal".to_owned(),
            },
            std::str::from_utf8(&res.stdout).unwrap_or("<invalid utf8>"),
            std::str::from_utf8(&res.stderr).unwrap_or("<invalid utf8>"),
        );
        Ok(())
    }
}
