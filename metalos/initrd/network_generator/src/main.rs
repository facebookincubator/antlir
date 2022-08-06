use anyhow::Context;
use anyhow::Result;
use kernel_cmdline::GenericCmdlineOpt;
use kernel_cmdline::KernelCmdArgs;
use kernel_cmdline::KnownArgs;
use net_utils::get_mac;
use network_generator_lib::generator_main;
use structopt::clap::AppSettings;
use structopt::StructOpt;
use strum_macros::EnumIter;

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
    pub mac_address: Option<String>,
}

impl KernelCmdArgs for NetworkGeneratorArgs {
    type Args = NetworkGeneratorKnownArgs;
}

fn main() -> Result<()> {
    generator_main(|| {
        let kargs = NetworkGeneratorArgs::from_proc_cmdline()
            .context("Failed to read kernel command line")?;

        Ok(match kargs.mac_address {
            Some(mac) => mac,
            None => get_mac()
                .context("Failed to auto-detect mac address (not provided on kernel cmdline)")?,
        })
    })
}
