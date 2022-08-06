use std::collections::BTreeSet;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use structopt::StructOpt;
use strum::IntoEnumIterator;

pub trait KnownArgs: IntoEnumIterator {
    fn flag_name(&self) -> &'static str;
}

pub trait KernelCmdArgs: StructOpt + Sized {
    type Args: KnownArgs;

    /// Parse /proc/cmdline to get values from the booted kernel cmdline. Some
    /// selected options are available when they have significance for MetalOS
    /// code, for example 'metalos.os_uri'.
    fn from_kernel_args(s: &str) -> Result<Self> {
        let known_args: BTreeSet<&'static str> =
            Self::Args::iter().map(|v| v.flag_name()).collect();

        let mut iter = shlex::Shlex::new(s);
        let mut args = Vec::new();
        for token in &mut iter {
            let (key, value) = match token.split_once('=') {
                Some((key, val)) => (key, Some(val)),
                None => (token.as_str(), None),
            };
            let key = key.replace('-', "_");
            let key = if known_args.contains(&format!("--{}", key).as_ref()) {
                format!("--{}", key)
            } else {
                key
            };

            args.push(match value {
                Some(value) => format!("{}={}", key, value),
                None => key,
            });
        }

        if iter.had_error {
            Err(anyhow!(
                "{} is invalid, successfully parsed {:?} so far",
                s,
                args,
            ))
        } else {
            Self::from_iter_safe(args).context("Failed to parse args")
        }
    }

    fn from_proc_cmdline() -> Result<Self> {
        Self::from_kernel_args(
            &std::fs::read_to_string("/proc/cmdline").context("failed to read /proc/cmdline")?,
        )
    }
}

#[derive(Debug, PartialEq)]
pub enum GenericCmdlineOpt {
    OnOff(String, bool),
    Kv(String, String),
}

impl GenericCmdlineOpt {
    pub fn parse_arg(src: &str) -> Self {
        match src.split_once('=') {
            Some((key, val)) => match val {
                "0" | "false" | "no" => Self::OnOff(key.to_string(), false),
                "1" | "true" | "yes" => Self::OnOff(key.to_string(), true),
                _ => Self::Kv(key.to_string(), val.to_owned()),
            },
            None => Self::OnOff(src.to_string(), true),
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use structopt::clap::AppSettings;
    use structopt::StructOpt;
    use strum_macros::EnumIter;

    use super::GenericCmdlineOpt;
    use super::KernelCmdArgs;
    use super::KnownArgs;

    #[derive(EnumIter)]
    enum KnownTestArgs {
        HostConfigUri,
        TestFlag1,
        TestFlag2,
    }

    impl KnownArgs for KnownTestArgs {
        fn flag_name(&self) -> &'static str {
            match self {
                Self::HostConfigUri => "--metalos.host_config_uri",
                Self::TestFlag1 => "--test_flag1",
                Self::TestFlag2 => "--test_flag2",
            }
        }
    }

    #[derive(Debug, StructOpt, PartialEq)]
    #[structopt(name = "kernel-cmdline-unittest", setting(AppSettings::NoBinaryName))]
    struct TestArgs {
        #[structopt(parse(from_str = GenericCmdlineOpt::parse_arg))]
        non_metalos_opts: Vec<GenericCmdlineOpt>,

        #[structopt(long = &KnownTestArgs::HostConfigUri.flag_name())]
        host_config_uri: Option<String>,

        #[structopt(long = &KnownTestArgs::TestFlag1.flag_name())]
        test_flag_1: Option<String>,

        #[structopt(long = &KnownTestArgs::TestFlag2.flag_name())]
        test_flag_2: Option<String>,
    }

    impl KernelCmdArgs for TestArgs {
        type Args = KnownTestArgs;
    }

    #[derive(Debug, StructOpt, PartialEq)]
    #[structopt(
        name = "kernel-cmdline-unittest-required",
        setting(AppSettings::NoBinaryName)
    )]
    struct TestRequiredArgs {
        #[structopt(parse(from_str = GenericCmdlineOpt::parse_arg))]
        non_metalos_opts: Vec<GenericCmdlineOpt>,

        #[structopt(long = &KnownTestArgs::HostConfigUri.flag_name())]
        host_config_uri: String,

        #[structopt(long = &KnownTestArgs::TestFlag1.flag_name())]
        test_flag_1: String,

        #[structopt(long = &KnownTestArgs::TestFlag2.flag_name())]
        test_flag_2: Option<String>,
    }

    impl KernelCmdArgs for TestRequiredArgs {
        type Args = KnownTestArgs;
    }

    #[test]
    fn url_value() -> Result<()> {
        let cmdline = TestArgs::from_kernel_args(
            "metalos.host-config-uri=\"https://$HOSTNAME:8000/v1/host/host001.01.abc0.facebook.com\"",
        )?;
        assert_eq!(
            cmdline,
            TestArgs {
                non_metalos_opts: Vec::new(),
                host_config_uri: Some(
                    "https://$HOSTNAME:8000/v1/host/host001.01.abc0.facebook.com".to_string()
                ),
                test_flag_1: None,
                test_flag_2: None,
            }
        );
        Ok(())
    }

    #[test]
    fn real_life_cmdline() -> Result<()> {
        let cmdline = TestArgs::from_kernel_args(
            "BOOT_IMAGE=(hd0,msdos1)/vmlinuz-5.2.9-229_fbk15_hardened_4185_g357f49b36602 ro root=LABEL=/ biosdevname=0 net.ifnames=0 fsck.repair=yes systemd.gpt_auto=0 ipv6.autoconf=0 erst_disable cgroup_no_v1=all nox2apic crashkernel=128M hugetlb_cma=6G console=tty0 console=ttyS0,115200 macaddress=11:22:33:44:55:66",
        )?;
        let kv = |key: &str, val: &str| GenericCmdlineOpt::Kv(key.to_owned(), val.to_owned());
        let on_off = |key: &str, val: bool| GenericCmdlineOpt::OnOff(key.to_owned(), val);
        assert_eq!(
            TestArgs {
                non_metalos_opts: vec![
                    kv(
                        "BOOT_IMAGE",
                        "(hd0,msdos1)/vmlinuz-5.2.9-229_fbk15_hardened_4185_g357f49b36602"
                    ),
                    on_off("ro", true),
                    kv("root", "LABEL=/"),
                    on_off("biosdevname", false),
                    on_off("net.ifnames", false),
                    on_off("fsck.repair", true),
                    on_off("systemd.gpt_auto", false),
                    on_off("ipv6.autoconf", false),
                    on_off("erst_disable", true),
                    kv("cgroup_no_v1", "all"),
                    on_off("nox2apic", true),
                    kv("crashkernel", "128M"),
                    kv("hugetlb_cma", "6G"),
                    kv("console", "tty0"),
                    kv("console", "ttyS0,115200"),
                    kv("macaddress", "11:22:33:44:55:66"),
                ],
                host_config_uri: None,
                test_flag_1: None,
                test_flag_2: None,
            },
            cmdline
        );
        Ok(())
    }

    #[test]
    fn test_required_args() -> Result<()> {
        let cmdline = TestRequiredArgs::from_kernel_args(
            "unknown_kwarg=test unknown_arg metalos.host_config_uri=test_uri test_flag1=f1",
        )
        .expect("Required args test command should parse correctly");

        assert_eq!(
            cmdline,
            TestRequiredArgs {
                non_metalos_opts: vec![
                    GenericCmdlineOpt::Kv("unknown_kwarg".to_string(), "test".to_string()),
                    GenericCmdlineOpt::OnOff("unknown_arg".to_string(), true),
                ],
                host_config_uri: "test_uri".to_string(),
                test_flag_1: "f1".to_string(),
                test_flag_2: None,
            }
        );

        assert!(
            TestRequiredArgs::from_kernel_args("uknown_kwarg=test uknown_arg test_flag1=f1")
                .is_err()
        );
        assert!(
            TestRequiredArgs::from_kernel_args(
                "uknown_kwarg=test uknown_arg host_config_uri=test_uri"
            )
            .is_err()
        );
        assert!(TestRequiredArgs::from_kernel_args("uknown_kwarg=test uknown_arg").is_err());

        Ok(())
    }
}
