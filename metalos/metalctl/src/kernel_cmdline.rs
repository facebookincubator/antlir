/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::fs;

use anyhow::{anyhow, Context, Error, Result};
use structopt::clap::AppSettings;
use structopt::StructOpt;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

use crate::config::{EventBackendBaseUri, PackageFormatUri};

// This enum looks a bit weird to have here but it plays an important role in
// ensuring correctness. The exhaustive match in `flag_name` means that we have a valid
// flag value defined for every enum variant and then by trusting that `EnumIter` is
// correct we can build a list of all known valid flags.
//
// This should make it impossible to ever have the logic in MetalosCmdline::FromStr ever
// be out of sync which flags exist (assuming we always use the enum variant in the structopt
// which should be easy to enforce).
#[derive(EnumIter)]
enum KnownArgs {
    OsPackage,
    HostConfigUri,
    PackageFormatUri,
    Root,
    RootFsType,
    RootFlags,
    RootFlagRo,
    RootFlagRw,
    EventBackendBaseUri,
}

impl KnownArgs {
    fn flag_name(&self) -> &'static str {
        match self {
            Self::OsPackage => "--metalos.os_package",
            Self::HostConfigUri => "--metalos.host_config_uri",
            Self::PackageFormatUri => "--metalos.package_format_uri",
            Self::Root => "--root",
            Self::RootFsType => "--rootfstype",
            Self::RootFlags => "--rootflags",
            Self::RootFlagRo => "--ro",
            Self::RootFlagRw => "--rw",
            Self::EventBackendBaseUri => "--metalos.event_backend_base_uri",
        }
    }
}

#[derive(Debug, StructOpt, PartialEq)]
#[structopt(name = "kernel-cmdline", setting(AppSettings::NoBinaryName))]
pub struct MetalosCmdline {
    #[structopt(parse(from_str = parse_opt))]
    non_metalos_opts: Vec<KernelCmdlineOpt>,

    #[structopt(long = &KnownArgs::OsPackage.flag_name())]
    pub os_package: Option<String>,

    #[structopt(long = &KnownArgs::HostConfigUri.flag_name())]
    pub host_config_uri: Option<String>,

    #[structopt(long = &KnownArgs::PackageFormatUri.flag_name())]
    pub package_format_uri: Option<PackageFormatUri>,

    #[structopt(long = &KnownArgs::EventBackendBaseUri.flag_name())]
    pub event_backend_base_uri: Option<EventBackendBaseUri>,

    #[structopt(flatten)]
    pub root: Root,
}

#[derive(Debug, StructOpt, PartialEq)]
pub struct Root {
    #[structopt(long = &KnownArgs::Root.flag_name())]
    pub root: Option<String>,

    #[structopt(long = &KnownArgs::RootFsType.flag_name())]
    pub fstype: Option<String>,

    #[structopt(long = &KnownArgs::RootFlags.flag_name())]
    flags: Option<Vec<String>>,

    #[structopt(long = &KnownArgs::RootFlagRo.flag_name())]
    ro: bool,

    #[structopt(long = &KnownArgs::RootFlagRw.flag_name())]
    rw: bool,
}

impl Root {
    fn get_flags(&self) -> Vec<String> {
        let mut flags = self.flags.clone().unwrap_or_else(Vec::new);
        if self.ro {
            flags.push("ro".to_string());
        }
        if self.rw {
            flags.push("rw".to_string());
        }

        flags
    }

    #[cfg_attr(not(initrd), allow(dead_code))]
    pub fn join_flags(&self) -> Option<String> {
        let flags = self.get_flags();
        match flags.is_empty() {
            true => None,
            false => Some(self.get_flags().join(",")),
        }
    }
}

impl MetalosCmdline {
    pub fn from_kernel() -> Result<Self> {
        fs::read_to_string("/proc/cmdline")
            .context("failed to read /proc/cmdline")?
            .parse()
    }
}

#[derive(Debug, PartialEq)]
enum KernelCmdlineOpt {
    OnOff(String, bool),
    Kv(String, String),
}

fn parse_opt(src: &str) -> KernelCmdlineOpt {
    match src.split_once("=") {
        Some((key, val)) => match val {
            "0" | "false" | "no" => KernelCmdlineOpt::OnOff(key.to_string(), false),
            "1" | "true" | "yes" => KernelCmdlineOpt::OnOff(key.to_string(), true),
            _ => KernelCmdlineOpt::Kv(key.to_string(), val.to_owned()),
        },
        None => KernelCmdlineOpt::OnOff(src.to_string(), true),
    }
}

impl std::str::FromStr for MetalosCmdline {
    type Err = Error;

    /// Parse /proc/cmdline to get values from the booted kernel cmdline. Some
    /// selected options are available when they have significance for MetalOS
    /// code, for example 'metalos.os_uri'.
    fn from_str(s: &str) -> Result<Self> {
        let known_args: BTreeSet<&'static str> = KnownArgs::iter().map(|v| v.flag_name()).collect();

        let mut iter = shlex::Shlex::new(s);
        let mut args = Vec::new();
        for token in &mut iter {
            let (key, value) = match token.split_once("=") {
                Some((key, val)) => (key, Some(val)),
                None => (token.as_str(), None),
            };
            let key = key.replace("-", "_");
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
}

#[cfg(test)]
mod tests {
    use super::{KnownArgs, MetalosCmdline, Root};
    use anyhow::{anyhow, Result};
    use std::collections::BTreeSet;
    use strum::IntoEnumIterator;

    #[test]
    fn test_known_args() -> Result<()> {
        let args_list: Vec<&str> = KnownArgs::iter().map(|v| v.flag_name()).collect();
        let args_set: BTreeSet<&str> = args_list.clone().into_iter().collect();

        if args_set.len() != args_list.len() {
            return Err(anyhow!(
                "Duplicate flag detected in KnownArgs:\n{:#?}",
                args_list
            ));
        }
        Ok(())
    }

    #[test]
    fn basic_cmdlines() -> Result<()> {
        for cmdline in &[
            "rd.systemd.debug_shell=1 quiet metalos.os_package=some-pkg:id",
            "rd.systemd.debug_shell=1 quiet metalos.os-package=some-pkg:id",
            "rd.systemd.debug_shell=1 quiet metalos.os_package=\"some-pkg:id\"",
        ] {
            let cmdline: MetalosCmdline = cmdline.parse()?;
            assert_eq!(Some("some-pkg:id".to_string()), cmdline.os_package);
        }
        Ok(())
    }

    #[test]
    fn url_value() -> Result<()> {
        let cmdline: MetalosCmdline =
            "metalos.host-config-uri=\"https://$HOSTNAME:8000/v1/host/host001.01.abc0.facebook.com\"".parse()?;
        assert_eq!(
            Some("https://$HOSTNAME:8000/v1/host/host001.01.abc0.facebook.com".to_string()),
            cmdline.host_config_uri
        );
        Ok(())
    }

    #[test]
    fn real_life_cmdline() -> Result<()> {
        let cmdline = "BOOT_IMAGE=(hd0,msdos1)/vmlinuz-5.2.9-229_fbk15_hardened_4185_g357f49b36602 ro root=LABEL=/ biosdevname=0 net.ifnames=0 fsck.repair=yes systemd.gpt_auto=0 ipv6.autoconf=0 erst_disable cgroup_no_v1=all nox2apic crashkernel=128M hugetlb_cma=6G console=tty0 console=ttyS0,115200".parse()?;
        let kv = |key: &str, val: &str| super::KernelCmdlineOpt::Kv(key.to_owned(), val.to_owned());
        let on_off = |key: &str, val: bool| super::KernelCmdlineOpt::OnOff(key.to_owned(), val);
        assert_eq!(
            MetalosCmdline {
                non_metalos_opts: vec![
                    kv(
                        "BOOT_IMAGE",
                        "(hd0,msdos1)/vmlinuz-5.2.9-229_fbk15_hardened_4185_g357f49b36602"
                    ),
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
                    kv("console", "ttyS0,115200")
                ],
                os_package: None,
                host_config_uri: None,
                package_format_uri: None,
                event_backend_base_uri: None,
                root: Root {
                    root: Some("LABEL=/".to_string()),
                    flags: None,
                    fstype: None,
                    ro: true,
                    rw: false,
                }
            },
            cmdline
        );
        assert_eq!(
            Root {
                root: Some("LABEL=/".to_string()),
                flags: None,
                ro: true,
                rw: false,
                fstype: None,
            },
            cmdline.root
        );
        Ok(())
    }

    #[test]
    fn cmdline_root() {
        assert_eq!(
            Root {
                root: Some("LABEL=/".to_string()),
                flags: Some(vec!["subvol=volume".to_string()]),
                fstype: Some("btrfs".to_string()),
                ro: true,
                rw: false,
            },
            "root=LABEL=/ ro rootflags=subvol=volume rootfstype=btrfs"
                .parse::<MetalosCmdline>()
                .unwrap()
                .root,
        );
    }
}
