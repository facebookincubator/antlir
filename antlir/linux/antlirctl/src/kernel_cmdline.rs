/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;

use anyhow::{Context, Error, Result};
use structopt::clap::AppSettings;
use structopt::StructOpt;

#[derive(Debug, StructOpt, PartialEq, Default)]
#[structopt(name = "kernel-cmdline", setting(AppSettings::NoBinaryName))]
pub struct AntlirCmdline {
    #[structopt(parse(from_str = parse_opt))]
    opts: Vec<KernelCmdlineOpt>,
}

#[derive(Debug, PartialEq)]
pub struct Root<'a> {
    pub root: &'a str,
    pub flags: Option<String>,
    pub fstype: Option<&'a str>,
}

impl AntlirCmdline {
    pub fn from_kernel() -> Result<AntlirCmdline> {
        fs::read_to_string("/proc/cmdline")
            .context("failed to read /proc/cmdline")?
            .parse()
    }

    fn arg(&self, key: &str) -> Option<&KernelCmdlineOpt> {
        self.opts.iter().find(|o| match o {
            KernelCmdlineOpt::OnOff(k, _) => k == key,
            KernelCmdlineOpt::Kv(k, _) => k == key,
        })
    }

    pub fn os_uri(&self) -> Option<&str> {
        self.arg("antlir.os_uri").and_then(|opt| opt.as_value())
    }

    pub fn root(&self) -> Option<Root> {
        self.arg("root")
            .and_then(|root| root.as_value())
            .map(|root| {
                let mut flags = self
                    .arg("rootflags")
                    .and_then(|opt| opt.as_value())
                    .map_or(vec![], |flags| flags.split(',').collect());
                if self
                    .arg("ro")
                    .and_then(|opt| opt.as_bool())
                    .unwrap_or(false)
                {
                    flags.push("ro");
                }
                if self
                    .arg("rw")
                    .and_then(|opt| opt.as_bool())
                    .unwrap_or(false)
                {
                    flags.push("rw");
                }
                let flags = flags.join(",");
                let flags = match flags.is_empty() {
                    true => None,
                    false => Some(flags),
                };
                Root {
                    root,
                    flags,
                    fstype: self.arg("rootfstype").and_then(|opt| opt.as_value()),
                }
            })
    }
}

#[derive(Debug, PartialEq)]
enum KernelCmdlineOpt {
    OnOff(String, bool),
    Kv(String, String),
}

impl KernelCmdlineOpt {
    fn as_value(&self) -> Option<&str> {
        match self {
            Self::Kv(_, val) => Some(val),
            _ => None,
        }
    }
    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::OnOff(_, val) => Some(*val),
            _ => None,
        }
    }
}

fn parse_opt(src: &str) -> KernelCmdlineOpt {
    match src.split_once("=") {
        Some((key, val)) => {
            // normalize dashes in keys to underscores
            let key = key.replace("-", "_");
            match val {
                "0" | "false" | "no" => KernelCmdlineOpt::OnOff(key, false),
                "1" | "true" | "yes" => KernelCmdlineOpt::OnOff(key, true),
                _ => KernelCmdlineOpt::Kv(key, val.to_owned()),
            }
        }
        None => KernelCmdlineOpt::OnOff(src.replace("-", "_"), true),
    }
}

#[derive(PartialEq)]
enum ParserState {
    Push,
    Quoted,
}

impl std::str::FromStr for AntlirCmdline {
    type Err = Error;

    /// Parse /proc/cmdline to get values from the booted kernel cmdline. Some
    /// selected options are available when they have significance for Antlir
    /// code, for example 'antlir.os_uri'.
    fn from_str(s: &str) -> Result<Self> {
        // strip leading and trailing whitespace
        let s = s.trim();
        let mut state = ParserState::Push;
        let mut args = vec![];
        let mut current = String::new();
        for ch in s.chars() {
            if ch.is_whitespace() {
                match state {
                    ParserState::Push => {
                        args.push(current);
                        current = String::new();
                    }
                    ParserState::Quoted => current.push(ch),
                };
            } else if ch == '"' {
                state = match state {
                    ParserState::Push => ParserState::Quoted,
                    ParserState::Quoted => ParserState::Push,
                }
            } else {
                current.push(ch);
            }
        }
        // last arg does not have the trailing whitespace to trigger the push in
        // the above loop
        args.push(current);
        eprintln!("{:?}", args);
        AntlirCmdline::from_iter_safe(args).context("failed to parse")
    }
}

#[cfg(test)]
mod tests {
    use super::{AntlirCmdline, Root};
    use anyhow::Result;

    #[test]
    fn basic_cmdlines() -> Result<()> {
        for cmdline in &[
            "rd.systemd.debug_shell=1 quiet antlir.os_uri=https://some/url",
            "rd.systemd.debug_shell=1 quiet antlir.os-uri=https://some/url",
            "rd.systemd.debug_shell=1 quiet antlir.os_uri=\"https://some/url\"",
        ] {
            let cmdline: AntlirCmdline = cmdline.parse()?;
            eprintln!("{:?}", cmdline);
            assert_eq!(Some("https://some/url"), cmdline.os_uri(),);
        }
        Ok(())
    }

    #[test]
    fn real_life_cmdline() -> Result<()> {
        let cmdline = "BOOT_IMAGE=(hd0,msdos1)/vmlinuz-5.2.9-229_fbk15_hardened_4185_g357f49b36602 ro root=LABEL=/ biosdevname=0 net.ifnames=0 fsck.repair=yes systemd.gpt_auto=0 ipv6.autoconf=0 erst_disable cgroup_no_v1=all nox2apic crashkernel=128M hugetlb_cma=6G console=tty0 console=ttyS0,115200".parse()?;
        let kv = |key: &str, val: &str| super::KernelCmdlineOpt::Kv(key.to_owned(), val.to_owned());
        let on_off = |key: &str, val: bool| super::KernelCmdlineOpt::OnOff(key.to_owned(), val);
        assert_eq!(
            AntlirCmdline {
                opts: vec![
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
                    kv("console", "ttyS0,115200")
                ],
            },
            cmdline
        );
        assert_eq!(
            Some(Root {
                root: "LABEL=/",
                flags: Some("ro".into()),
                fstype: None,
            }),
            cmdline.root()
        );
        Ok(())
    }

    #[test]
    fn cmdline_root() {
        assert_eq!(
            Some(Root {
                root: "LABEL=/",
                flags: Some("subvol=volume,ro".to_string()),
                fstype: Some("btrfs"),
            }),
            "root=LABEL=/ ro rootflags=subvol=volume rootfstype=btrfs"
                .parse::<AntlirCmdline>()
                .unwrap()
                .root(),
        );
    }
}
