/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This file contains data structure that mirrors what described in vm bzl files
//! so that we can directly deserialize a json into Rust structs.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::PathBuf;

use clap::Args;
use image_test_lib::KvPair;
use serde::Deserialize;

/// Captures property of the disk specified by user to describe a writable disk
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct QCow2DiskOpts {
    /// Path to the base image file
    pub(crate) base_image: Option<PathBuf>,
    /// Resize the disk to provide additional space. This will also be size of entire
    /// disk if `base_image` was not given.
    pub(crate) additional_mib: Option<usize>,
    /// Corresponds to driver for -device qemu arg. For disks, this is the interface.
    /// Examples: virtio-blk, nvme
    pub(crate) interface: String,
    /// Physical block size of the disk
    pub(crate) physical_block_size: usize,
    /// Logical block size of the disk
    pub(crate) logical_block_size: usize,
}

/// Required data if not booting from disk
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct NonDiskBootOpts {
    /// Path to initrd
    pub(crate) initrd: String,
    /// Path to kernel
    pub(crate) kernel: String,
    /// Additional kernel parameters to append
    #[serde(default)]
    pub(crate) append: String,
}

/// `ShareOpts` describes the property of a shared directory.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct ShareOpts {
    /// Path to the directory to share
    pub(crate) path: PathBuf,
    /// Read-only mount if true. R/W otherwise.
    pub(crate) read_only: bool,
    /// Mount tag override. If None, a unique tag will be generated
    pub(crate) mount_tag: Option<String>,
}

/// Operational specific parameters for VM but not related to VM configuration itself
#[derive(Debug, Clone, Args)]
pub(crate) struct VMArgs {
    /// Timeout in seconds before VM will be terminated. None disables the
    /// timeout, which should only be used for interactive shells for
    /// development.
    #[clap(long)]
    pub(crate) timeout_s: Option<u32>,
    /// Show console outputs. Disabled by default.
    #[clap(long)]
    pub(crate) console: bool,
    /// Additional writable directories for outputs
    #[clap(long)]
    pub(crate) output_dirs: Vec<PathBuf>,
    /// Environment variables for the command
    #[clap(long)]
    pub(crate) command_envs: Option<Vec<KvPair>>,
    /// Command to execute inside VM. If not passed in, a shell will be spawned
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) command: Option<Vec<OsString>>,
}

impl VMArgs {
    /// Generate list of args that can be parsed again by clap to yield
    /// the same content as `self`.
    pub(crate) fn to_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = Vec::new();
        if let Some(timeout_s) = &self.timeout_s {
            args.push("--timeout-s".into());
            args.push(timeout_s.to_string().into());
        }
        if self.console {
            args.push("--console".into());
        }
        if let Some(command_envs) = &self.command_envs {
            for pair in command_envs {
                args.push("--command-envs".into());
                let mut kv_str = OsString::new();
                kv_str.push(pair.key.clone());
                kv_str.push(OsStr::new("="));
                kv_str.push(pair.value.clone());
                args.push(kv_str);
            }
        }
        self.output_dirs.iter().for_each(|dir| {
            args.push("--output-dirs".into());
            args.push(dir.clone().into());
        });
        if let Some(command) = &self.command {
            command.iter().for_each(|c| args.push(c.clone()));
        }
        args
    }
}

/// Everything we need to create and run the VM
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MachineOpts {
    /// number of cores
    pub(crate) cpus: usize,
    /// memory size in MiB
    pub(crate) mem_mib: usize,
    /// List of writable disks. We expect at least one disk and the first one
    /// would be the root disk.
    pub(crate) disks: Vec<QCow2DiskOpts>,
    /// Number of NICs for the VM.
    pub(crate) num_nics: usize,
    /// initrd and data if not booting from disk
    pub(crate) non_disk_boot_opts: Option<NonDiskBootOpts>,
}

/// Location of various binary and data we need to operate the VM
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct RuntimeOpts {
    pub(crate) qemu_system: String,
    pub(crate) qemu_img: String,
    pub(crate) firmware: String,
    pub(crate) roms_dir: String,
}

#[cfg(test)]
mod test {
    use clap::Parser;

    use super::*;

    #[test]
    fn test_vmargs_to_args() {
        #[derive(Debug, Parser)]
        struct TestArgs {
            #[clap(flatten)]
            args: VMArgs,
        }

        [
            vec!["bin"],
            vec!["bin", "--console"],
            vec!["bin", "--timeout-s", "10"],
            vec!["bin", "--output-dirs", "/foo", "--output-dirs", "/bar"],
            vec![
                "bin",
                "--command-envs",
                "foo=bar",
                "--command-envs",
                "bar=foo",
            ],
            vec!["bin", "hello"],
        ]
        .iter()
        .for_each(|args| {
            let parsed = TestArgs::parse_from(args).args;
            let original: Vec<_> = args.iter().skip(1).map(OsString::from).collect();
            assert_eq!(parsed.to_args(), original);
        });

        // Tests for `command` to ensure we carry over flags correctly for common
        // pattern used by tests
        [
            vec!["bin", "hello", "world"],
            vec!["bin", "--hello", "world"],
            vec!["bin", "omg", "--hello", "world"],
            vec!["bin", "omg", "--hello", "world", "whatever"],
        ]
        .iter()
        .for_each(|args| {
            let parsed = TestArgs::parse_from(args).args;
            let original: Vec<_> = args.iter().skip(1).map(OsString::from).collect();
            assert_eq!(
                &parsed
                    .command
                    .clone()
                    .expect("command field shouldn't be None"),
                &original,
            );
            assert_eq!(parsed.to_args(), original);
        });
    }
}
