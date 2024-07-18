/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use antlir2_cas_dir::CasDirOpts;
use anyhow::Context;
use anyhow::Result;
use nix::unistd::Gid;
use nix::unistd::Uid;
use serde::Deserialize;

use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CasDir {
    layer: PathBuf,
}

impl PackageFormat for CasDir {
    fn build(&self, out: &Path) -> Result<()> {
        antlir2_cas_dir::CasDir::dehydrate(
            &self.layer,
            out.to_owned(),
            CasDirOpts::default()
                .uid(match std::env::var("SUDO_UID") {
                    Err(_) => Uid::current(),
                    Ok(id) => Uid::from_raw(id.parse().context("while parsing SUDO_UID")?),
                })
                .gid(match std::env::var("SUDO_GID") {
                    Err(_) => Gid::current(),
                    Ok(id) => Gid::from_raw(id.parse().context("while parsing SUDO_GID")?),
                }),
        )?;
        Ok(())
    }
}
