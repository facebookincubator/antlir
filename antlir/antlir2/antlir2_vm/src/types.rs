/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This file contains data structure that mirrors what described in vm bzl files
//! so that we can directly deserialize a json into Rust structs.

use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use clap::Args;
use image_test_lib::KvPair;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum TypeError {
    #[error("Failed to parse CpuIsa from string: {0}")]
    InvalidCpuIsa(String),
}

/// Public interface for implementing a Qemu device
pub(crate) trait QemuDevice {
    /// Returns a list of qemu args that can be joined with others to eventually
    /// spawn the qemu process
    fn qemu_args(&self) -> Vec<OsString>;
}

/// Captures property of the disk specified by user to describe a writable disk
#[derive(Debug, Clone, Default, Deserialize)]
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
    /// Device serial override. By default it's automatically assigned.
    pub(crate) serial: Option<String>,
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
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
pub(crate) struct ShareOpts {
    /// Path to the directory to share
    pub(crate) path: PathBuf,
    /// Read-only mount if true. R/W otherwise.
    pub(crate) read_only: bool,
    /// Mount tag override. If None, a unique tag will be generated
    pub(crate) mount_tag: Option<String>,
}

/// Operational specific parameters for VM but not related to VM configuration itself
#[derive(Debug, Clone, Args, PartialEq, Default)]
pub(crate) struct VMArgs {
    /// Timeout in seconds before VM will be terminated. None disables the
    /// timeout, which should only be used for interactive shells for
    /// development.
    #[clap(long)]
    pub(crate) timeout_secs: Option<u32>,
    /// Redirect console output to file. By default it's suppressed.
    #[clap(long)]
    pub(crate) console_output_file: Option<PathBuf>,
    /// Output directories that need to be available inside VM
    #[clap(long)]
    pub(crate) output_dirs: Vec<PathBuf>,
    /// Environment variables for the command
    #[clap(long)]
    pub(crate) command_envs: Vec<KvPair>,
    /// Command requires first boot
    #[clap(long)]
    pub(crate) first_boot_command: Option<String>,
    /// Operation for VM to carry out
    #[clap(flatten)]
    pub(crate) mode: VMModeArgs,
}

/// Describes which VM mode to use. By default, an ssh shell into VM will open
/// after VM boots.
#[derive(Debug, Clone, Args, PartialEq, Default)]
#[group(multiple = false)]
pub(crate) struct VMModeArgs {
    /// Drop into console prompt. This also enables console output on screen,
    /// unless `--console-output-file` is specified.
    #[clap(long)]
    pub(crate) console: bool,
    /// Drop into container shell outside VM.
    #[clap(long)]
    pub(crate) container: bool,
    /// Execute command through ssh inside VM.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) command: Option<Vec<OsString>>,
}

impl VMArgs {
    /// Generate list of args that can be parsed again by clap to yield
    /// the same content as `self`.
    pub(crate) fn to_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = Vec::new();
        if let Some(timeout_secs) = &self.timeout_secs {
            args.push("--timeout-secs".into());
            args.push(timeout_secs.to_string().into());
        }
        if let Some(path) = &self.console_output_file {
            args.push("--console-output-file".into());
            args.push(path.into());
        }
        self.command_envs.iter().for_each(|pair| {
            args.push("--command-envs".into());
            let mut kv_str = OsString::new();
            kv_str.push(pair.key.clone());
            kv_str.push(OsStr::new("="));
            kv_str.push(pair.value.clone());
            args.push(kv_str);
        });
        if let Some(first_boot_command) = &self.first_boot_command {
            args.push("--first-boot-command".into());
            args.push(first_boot_command.into());
        }
        self.output_dirs.iter().for_each(|dir| {
            args.push("--output-dirs".into());
            args.push(dir.clone().into());
        });
        if self.mode.console {
            args.push("--console".into());
        }
        if self.mode.container {
            args.push("--container".into());
        }
        if let Some(command) = &self.mode.command {
            command.iter().for_each(|c| args.push(c.clone()));
        }
        args
    }

    /// Get all output directories for the VM.
    pub(crate) fn get_vm_output_dirs(&self) -> HashSet<PathBuf> {
        let outputs: HashSet<_> = self.output_dirs.iter().cloned().collect();
        outputs
    }

    /// Get all output directories for the container.
    pub(crate) fn get_container_output_dirs(&self) -> HashSet<PathBuf> {
        let mut outputs = self.get_vm_output_dirs();
        // Console output needs to be accessible for debugging and uploading
        if let Some(file_path) = &self.console_output_file {
            if let Some(parent) = file_path.parent() {
                outputs.insert(parent.to_path_buf());
            } else {
                outputs.insert(env::current_dir().expect("current dir must be valid"));
            }
        }
        // Carry over virtualization support
        outputs.insert("/dev/kvm".into());
        outputs
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub(crate) enum CpuIsa {
    #[serde(rename = "aarch64")]
    AARCH64,
    #[default]
    #[serde(rename = "x86_64")]
    X86_64,
}

impl fmt::Display for CpuIsa {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::X86_64 => write!(f, "x86_64"),
            Self::AARCH64 => write!(f, "aarch64"),
        }
    }
}

impl FromStr for CpuIsa {
    type Err = TypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "x86_64" => Ok(Self::X86_64),
            "aarch64" => Ok(Self::AARCH64),
            _ => Err(TypeError::InvalidCpuIsa(s.to_owned())),
        }
    }
}

/// Everything we need to create and run the VM
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct MachineOpts {
    /// ISA of the emulated machine
    pub(crate) arch: CpuIsa,
    /// Number of cores
    pub(crate) cpus: usize,
    /// Memory size in MiB
    pub(crate) mem_mib: usize,
    /// List of writable disks. We expect at least one disk and the first one
    /// would be the root disk.
    pub(crate) disks: Vec<QCow2DiskOpts>,
    /// Number of NICs for the VM.
    pub(crate) num_nics: usize,
    /// Maximum number of combined channels for each virtual NIC. Setting it to 1 disables multi-queue
    pub(crate) max_combined_channels: usize,
    /// initrd and data if not booting from disk
    pub(crate) non_disk_boot_opts: Option<NonDiskBootOpts>,
    /// Index of serial port
    pub(crate) serial_index: usize,
    /// Processes that will spawn outside VM that VM can communicate with
    pub(crate) sidecar_services: Vec<Vec<String>>,
    /// Enables TPM 2.0 support
    pub(crate) use_tpm: bool,
    /// Use 9p instead of virtiofs for sharing. This is required for kernel older than 5.4.
    pub(crate) use_legacy_share: bool,
}

/// Location of various binary and data we need to operate the VM
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct RuntimeOpts {
    pub(crate) qemu_system: String,
    pub(crate) qemu_img: String,
    pub(crate) firmware: String,
    pub(crate) roms_dir: String,
    pub(crate) swtpm: String,
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
            vec!["bin", "--container"],
            vec!["bin", "--console-output-file", "/path/to/out"],
            vec!["bin", "--timeout-secs", "10"],
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
                    .mode
                    .command
                    .clone()
                    .expect("command field shouldn't be None"),
                &original,
            );
            assert_eq!(parsed.to_args(), original);
        });
    }

    #[test]
    fn test_get_vm_output_dirs() {
        let args = VMArgs::default();
        assert!(args.get_vm_output_dirs().is_empty());
        let args = VMArgs {
            output_dirs: vec!["/foo/bar".into(), "/baz".into()],
            ..Default::default()
        };
        assert_eq!(
            args.get_vm_output_dirs(),
            HashSet::from(["/foo/bar".into(), "/baz".into()])
        );
        let args = VMArgs {
            output_dirs: vec!["/foo/bar".into()],
            console_output_file: Some("/tmp/whatever".into()),
            ..Default::default()
        };
        assert_eq!(
            args.get_vm_output_dirs(),
            HashSet::from(["/foo/bar".into()])
        );
    }

    #[test]
    fn test_get_container_output_dirs() {
        let args = VMArgs::default();
        assert_eq!(
            args.get_container_output_dirs(),
            HashSet::from(["/dev/kvm".into()])
        );
        let args = VMArgs {
            output_dirs: vec!["/foo/bar".into(), "/baz".into()],
            console_output_file: Some("/tmp/whatever".into()),
            ..Default::default()
        };
        assert_eq!(
            args.get_container_output_dirs(),
            HashSet::from([
                "/foo/bar".into(),
                "/baz".into(),
                "/tmp".into(),
                "/dev/kvm".into()
            ])
        );
    }
}
