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
    RootDiskPackage,
    HostConfigUri,
    PackageFormatUri,
    Root,
    RootFsType,
    RootFlags,
    RootFlagRo,
    RootFlagRw,
    EventBackendBaseUri,
    MacAddress,
}

impl KnownArgs for MetalCtlArgs {
    fn flag_name(&self) -> &'static str {
        match self {
            Self::RootDiskPackage => "--metalos.write_root_disk_package",
            Self::HostConfigUri => "--metalos.host_config_uri",
            Self::PackageFormatUri => "--metalos.package_format_uri",
            Self::Root => "--root",
            Self::RootFsType => "--rootfstype",
            Self::RootFlags => "--rootflags",
            Self::RootFlagRo => "--ro",
            Self::RootFlagRw => "--rw",
            Self::EventBackendBaseUri => "--metalos.event_backend_base_uri",
            Self::MacAddress => "--macaddress",
        }
    }
}

#[derive(Debug, StructOpt, PartialEq)]
#[structopt(name = "kernel-cmdline", setting(AppSettings::NoBinaryName))]
pub struct MetalosCmdline {
    #[structopt(parse(from_str = GenericCmdlineOpt::parse_arg))]
    non_metalos_opts: Vec<GenericCmdlineOpt>,

    #[structopt(long = &MetalCtlArgs::RootDiskPackage.flag_name())]
    pub root_disk_package: Option<String>,

    #[structopt(long = &MetalCtlArgs::HostConfigUri.flag_name())]
    pub host_config_uri: Option<String>,

    #[structopt(long = &MetalCtlArgs::PackageFormatUri.flag_name())]
    pub package_format_uri: Option<String>,

    #[structopt(long = &MetalCtlArgs::EventBackendBaseUri.flag_name())]
    pub event_backend_base_uri: Option<EventBackendBaseUri>,

    #[structopt(flatten)]
    pub root: Root,

    #[structopt(long = &MetalCtlArgs::MacAddress.flag_name())]
    pub mac_address: Option<String>,
}

impl KernelCmdArgs for MetalosCmdline {
    type Args = MetalCtlArgs;
}

#[derive(Debug, StructOpt, PartialEq)]
pub struct Root {
    #[structopt(long = &MetalCtlArgs::Root.flag_name())]
    pub root: Option<String>,

    #[structopt(long = &MetalCtlArgs::RootFsType.flag_name())]
    pub fstype: Option<String>,

    #[structopt(long = &MetalCtlArgs::RootFlags.flag_name())]
    pub(crate) flags: Option<Vec<String>>,

    #[structopt(long = &MetalCtlArgs::RootFlagRo.flag_name())]
    pub(crate) ro: bool,

    #[structopt(long = &MetalCtlArgs::RootFlagRw.flag_name())]
    pub(crate) rw: bool,
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

#[cfg(test)]
mod tests {
    use super::{MetalCtlArgs, MetalosCmdline, Root};
    use anyhow::{anyhow, Result};
    use kernel_cmdline::{GenericCmdlineOpt, KernelCmdArgs, KnownArgs};
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

    #[test]
    fn test_mac_address_in_cmdlines() -> Result<()> {
        let cmdline = MetalosCmdline::from_kernel_args("macaddress=11:22:33:44:55:66")?;
        assert_eq!(Some("11:22:33:44:55:66".to_string()), cmdline.mac_address);
        Ok(())
    }

    #[test]
    fn url_value() -> Result<()> {
        let cmdline = MetalosCmdline::from_kernel_args(
            "metalos.host-config-uri=\"https://$HOSTNAME:8000/v1/host/host001.01.abc0.facebook.com\"",
        )?;
        assert_eq!(
            Some("https://$HOSTNAME:8000/v1/host/host001.01.abc0.facebook.com".to_string()),
            cmdline.host_config_uri
        );
        Ok(())
    }

    #[test]
    fn real_life_cmdline() -> Result<()> {
        let cmdline = MetalosCmdline::from_kernel_args(
            "BOOT_IMAGE=(hd0,msdos1)/vmlinuz-5.2.9-229_fbk15_hardened_4185_g357f49b36602 ro root=LABEL=/ biosdevname=0 net.ifnames=0 fsck.repair=yes systemd.gpt_auto=0 ipv6.autoconf=0 erst_disable cgroup_no_v1=all nox2apic crashkernel=128M hugetlb_cma=6G console=tty0 console=ttyS0,115200 macaddress=11:22:33:44:55:66",
        )?;
        let kv = |key: &str, val: &str| GenericCmdlineOpt::Kv(key.to_owned(), val.to_owned());
        let on_off = |key: &str, val: bool| GenericCmdlineOpt::OnOff(key.to_owned(), val);
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
                root_disk_package: None,
                host_config_uri: None,
                package_format_uri: None,
                event_backend_base_uri: None,
                mac_address: Some("11:22:33:44:55:66".to_string()),
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
            MetalosCmdline::from_kernel_args(
                "root=LABEL=/ ro rootflags=subvol=volume rootfstype=btrfs"
            )
            .unwrap()
            .root,
        );
    }
}
