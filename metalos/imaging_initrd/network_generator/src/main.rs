use anyhow::Context;
use anyhow::Result;
use structopt::clap::AppSettings;
use structopt::StructOpt;
use strum_macros::EnumIter;

use kernel_cmdline::GenericCmdlineOpt;
use kernel_cmdline::KernelCmdArgs;
use kernel_cmdline::KnownArgs;
use network_generator_lib::generator_main;

#[derive(EnumIter)]
pub enum NetworkGeneratorKnownArgs {
    MacAddress,
}

impl KnownArgs for NetworkGeneratorKnownArgs {
    fn flag_name(&self) -> &'static str {
        match self {
            Self::MacAddress => "--macaddress",
        }
    }
}

#[derive(Debug, StructOpt)]
#[structopt(name = "kernel-cmdline", setting(AppSettings::NoBinaryName))]
pub struct NetworkGeneratorArgs {
    #[structopt(parse(from_str = GenericCmdlineOpt::parse_arg))]
    #[allow(dead_code)]
    non_metalos_opts: Vec<GenericCmdlineOpt>,

    #[structopt(long = &NetworkGeneratorKnownArgs::MacAddress.flag_name())]
    pub mac_address: String,
}

impl KernelCmdArgs for NetworkGeneratorArgs {
    type Args = NetworkGeneratorKnownArgs;
}

fn main() -> Result<()> {
    generator_main(|| {
        Ok(NetworkGeneratorArgs::from_proc_cmdline()
            .context("Failed to read kernel command line")?
            .mac_address)
    })
}
