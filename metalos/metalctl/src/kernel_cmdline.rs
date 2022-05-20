/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use structopt::clap::AppSettings;
use structopt::StructOpt;
use strum_macros::EnumIter;

use kernel_cmdline::{GenericCmdlineOpt, KernelCmdArgs, KnownArgs};

use crate::config::EventBackendBaseUri;

// This enum looks a bit weird to have here but it plays an important role in
// ensuring correctness. The exhaustive match in `flag_name` means that we have a valid
// flag value defined for every enum variant and then by trusting that `EnumIter` is
// correct we can build a list of all known valid flags.
//
// This should make it impossible to ever have the logic in MetalosCmdline::FromStr ever
// be out of sync which flags exist (assuming we always use the enum variant in the structopt
// which should be easy to enforce).
#[derive(EnumIter)]
pub enum MetalCtlArgs {
    EventBackendBaseUri,
}

impl KnownArgs for MetalCtlArgs {
    fn flag_name(&self) -> &'static str {
        match self {
            Self::EventBackendBaseUri => "--metalos.event_backend_base_uri",
        }
    }
}

#[derive(Debug, StructOpt, PartialEq)]
#[structopt(name = "kernel-cmdline", setting(AppSettings::NoBinaryName))]
pub struct MetalosCmdline {
    #[structopt(parse(from_str = GenericCmdlineOpt::parse_arg))]
    non_metalos_opts: Vec<GenericCmdlineOpt>,

    #[structopt(long = &MetalCtlArgs::EventBackendBaseUri.flag_name())]
    pub event_backend_base_uri: Option<EventBackendBaseUri>,
}

impl KernelCmdArgs for MetalosCmdline {
    type Args = MetalCtlArgs;
}

#[cfg(test)]
mod tests {
    use super::MetalCtlArgs;
    use anyhow::{anyhow, Result};
    use kernel_cmdline::KnownArgs;
    use std::collections::BTreeSet;
    use strum::IntoEnumIterator;

    #[test]
    fn test_known_args() -> Result<()> {
        let args_list: Vec<&str> = MetalCtlArgs::iter().map(|v| v.flag_name()).collect();
        let args_set: BTreeSet<&str> = args_list.clone().into_iter().collect();

        if args_set.len() != args_list.len() {
            return Err(anyhow!(
                "Duplicate flag detected in MetalCtlArgs:\n{:#?}",
                args_list
            ));
        }
        Ok(())
    }
}
