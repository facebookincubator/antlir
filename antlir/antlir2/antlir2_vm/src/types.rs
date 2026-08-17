/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This file contains data structure that mirrors what described in vm bzl files
//! so that we can directly deserialize a json into Rust structs.

use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fmt;
use std::path::Path;
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

/// Guest console log, inside [`VMArgs::logs_dir`].
pub(crate) const CONSOLE_LOG: &str = "console.txt";
/// Dump of the iSCSI connections still open once the VM has exited, inside
/// [`VMArgs::logs_dir`]. Anything in here means the guest failed to log out.
pub(crate) const ISCSI_LINGERING_CONNECTIONS_LOG: &str = "iscsi-lingering-connections.log";

/// Log file a host-side process writes to, e.g. `tgtd` -> `tgtd.log`.
pub(crate) fn log_file_name(binary: &str) -> String {
    format!("{binary}.log")
}

/// Buck encodes a sidecar service as a single space-separated command string
/// like `/path/to/binary arg1 arg2`, but its log file is named after the binary
/// alone, so drop both the arguments and the directory.
pub(crate) fn sidecar_binary_name(cmd_str: &str) -> &str {
    let first_token = cmd_str.split_whitespace().next().unwrap_or("unknown");
    Path::new(first_token)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(first_token)
}

/// A log file written on the host, outside the VM.
pub(crate) struct HostLogFile {
    /// File name within [`VMArgs::logs_dir`]
    pub(crate) name: String,
    /// Human readable description, used to annotate the tpx artifact
    pub(crate) description: String,
}

/// Public interface for implementing a Qemu device
pub(crate) trait QemuDevice {
    /// Returns a list of qemu args that can be joined with others to eventually
    /// spawn the qemu process
    fn qemu_args(&self) -> Vec<OsString>;
}

/// Captures property of the disk specified by user to describe a writable disk.
/// Deserialized from the machine spec JSON produced by Buck, where the
/// `interface` field acts as the discriminant and interface-specific options
/// live in a nested object (e.g. `"nvme": {"num_namespaces": 2}`).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "interface", rename_all = "kebab-case")]
pub(crate) enum QCow2DiskOpts {
    IdeHd(QCow2DiskCommonOpts),
    VirtioBlk(QCow2DiskCommonOpts),
    Nvme(QCow2DiskNvmeOpts),
    Iscsi(QCow2DiskIscsiOpts),
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct QCow2DiskCommonOpts {
    /// Path to the base image file
    pub(crate) base_image: Option<PathBuf>,
    /// Resize the disk to provide additional space. This will also be size of entire
    /// disk if `base_image` was not given.
    pub(crate) free_mib: Option<usize>,
    /// Physical block size of the disk
    pub(crate) physical_block_size: usize,
    /// Logical block size of the disk
    pub(crate) logical_block_size: usize,
    /// Device serial override. By default it's automatically assigned.
    pub(crate) serial: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct QCow2DiskNvmeOpts {
    #[serde(flatten)]
    pub(crate) common: QCow2DiskCommonOpts,
    pub(crate) nvme: NvmeOpts,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct NvmeOpts {
    pub(crate) num_namespaces: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct QCow2DiskIscsiOpts {
    #[serde(flatten)]
    pub(crate) common: QCow2DiskCommonOpts,
    pub(crate) iscsi: IscsiOpts,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IscsiOpts {
    #[serde(default)]
    pub(crate) ibft: bool,
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
    /// Directory holding every host-side log for this VM: the guest console
    /// ([`CONSOLE_LOG`]), `tgtd.log` and [`ISCSI_LINGERING_CONNECTIONS_LOG`]
    /// for iSCSI-backed disks, and `<binary>.log` per sidecar service. The
    /// whole directory is bind-mounted into the container and gets tpx artifact
    /// treatment, and postmortem commands find it at `$SIDECAR_LOGS_DIR`.
    #[clap(long)]
    pub(crate) logs_dir: Option<PathBuf>,
    /// Output directories that need to be available inside VM
    #[clap(long)]
    pub(crate) output_dirs: Vec<PathBuf>,
    /// Environment variables for the command
    #[clap(long)]
    pub(crate) command_envs: Vec<KvPair>,
    /// Command requires first boot
    #[clap(long)]
    pub(crate) first_boot_command: Option<String>,
    /// Dump network traffic on eth0 to output to file. By default it is not dumped.
    #[clap(long)]
    pub(crate) eth0_output_file: Option<PathBuf>,
    /// Pass credentials to systemd pid1
    #[arg(long)]
    pub(crate) systemd_credential: Vec<KvPair>,
    /// After the SSH command exits, wait for the VM process to exit on its
    /// own within this many seconds. Useful for verifying that a
    /// guest-initiated shutdown (e.g., `poweroff`) actually causes QEMU to
    /// terminate via ACPI S5. Leave unset to disable.
    #[clap(long)]
    pub(crate) expect_vm_exit: Option<u32>,
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
    /// unless `--logs-dir` is specified.
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
        if let Some(path) = &self.logs_dir {
            args.push("--logs-dir".into());
            args.push(path.into());
        }
        if let Some(path) = &self.eth0_output_file {
            args.push("--eth0-output-file".into());
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
        for pair in &self.systemd_credential {
            args.push("--systemd-credential".into());
            let mut kv_str = OsString::new();
            kv_str.push(pair.key.clone());
            kv_str.push(OsStr::new("="));
            kv_str.push(pair.value.clone());
            args.push(kv_str);
        }
        if let Some(expect_vm_exit) = self.expect_vm_exit {
            args.push("--expect-vm-exit".into());
            args.push(expect_vm_exit.to_string().into());
        }
        if let Some(command) = &self.mode.command {
            command.iter().for_each(|c| args.push(c.clone()));
        }
        args
    }

    /// Path of the guest console log inside [`Self::logs_dir`].
    pub(crate) fn console_output_file(&self) -> Option<PathBuf> {
        self.logs_dir.as_ref().map(|dir| dir.join(CONSOLE_LOG))
    }

    /// Get all output directories for the VM.
    pub(crate) fn get_vm_output_dirs(&self) -> HashSet<PathBuf> {
        let outputs: HashSet<_> = self.output_dirs.iter().cloned().collect();
        outputs
    }

    /// Get all output directories for the container.
    pub(crate) fn get_container_output_dirs(&self) -> HashSet<PathBuf> {
        let mut outputs = self.get_vm_output_dirs();
        // Every host-side log lands here, so it must be writable from inside
        // the container.
        if let Some(dir) = &self.logs_dir {
            outputs.insert(dir.clone());
        }
        // eth0 output needs to be accessible for debugging and uploading
        if let Some(file_path) = &self.eth0_output_file {
            if let Some(parent) = file_path.parent() {
                outputs.insert(parent.to_path_buf());
            } else {
                outputs.insert(env::current_dir().expect("current dir must be valid"));
            }
        }
        outputs
    }
}

#[derive(Debug, Copy, Clone, Default, Deserialize, PartialEq)]
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

/// Mount runtime platform (aka /usr/local/fbcode) from the host.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct MountPlatformDecision(pub(crate) bool);

/// Everything we need to create and run the VM
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Mount runtime platform (aka /usr/local/fbcode) from the host.
    pub(crate) mount_platform: MountPlatformDecision,
    /// initrd and data if not booting from disk
    pub(crate) non_disk_boot_opts: Option<NonDiskBootOpts>,
    /// Index of serial port
    pub(crate) serial_index: usize,
    /// Processes that will spawn outside VM that VM can communicate with
    pub(crate) sidecar_services: Vec<Vec<String>>,
    /// Enables TPM 2.0 support
    pub(crate) use_tpm: bool,
    /// Credential KV pairs for systemd
    pub(crate) systemd_credentials: HashMap<String, String>,
    /// Path to the QEMU binary. This is always set from Buck with architecture-specific
    /// defaults (qemu-system-x86_64 for x86_64, qemu-system-aarch64 for aarch64).
    pub(crate) qemu_binary: String,
    /// QEMU machine type (e.g., "q35", "virt", "microvm").
    /// This is always set from Buck with architecture-specific defaults
    /// (pc for x86_64, virt for aarch64).
    pub(crate) machine_type: String,
    /// Custom firmware binary path. When set, uses `-bios` instead of the
    /// default OVMF pflash. Use for Stage0 or other non-UEFI firmware.
    #[serde(default)]
    pub(crate) firmware: Option<String>,
    /// Additional raw QEMU arguments appended after all generated arguments.
    /// Useful for custom devices, chardevs, netdevs, etc. that are not
    /// natively supported by the VM framework (e.g., fbnic PCIe devices).
    /// Each arg is serialized as a single-element list by Buck2's attrs.arg().
    #[serde(default)]
    pub(crate) extra_qemu_args: Vec<Vec<String>>,
    /// Additional read-only host directories to bind-mount into the VM
    /// container. Each path is serialized as a single-element list by
    /// Buck2's attrs.arg().
    #[serde(default)]
    pub(crate) input_dirs: Vec<Vec<String>>,
    /// Additional read-write host directories to bind-mount into the VM
    /// container. Each path is serialized as a single-element list by
    /// Buck2's attrs.arg().
    #[serde(default)]
    pub(crate) output_dirs: Vec<Vec<String>>,
}

impl MachineOpts {
    /// True if any disk is iSCSI-backed, which means `tgtd` runs on the host.
    fn has_iscsi_disks(&self) -> bool {
        self.disks
            .iter()
            .any(|disk| matches!(disk, QCow2DiskOpts::Iscsi(_)))
    }

    /// Log file for each sidecar service, in `sidecar_services` order, paired
    /// with the binary that writes it. Naming a log after its binary is what
    /// lets a postmortem test find it, but the same binary can legitimately run
    /// more than once (e.g. one per port), so repeats get a `-<n>` suffix
    /// instead of clobbering the first instance's output.
    pub(crate) fn sidecar_logs(&self) -> Vec<(&str, String)> {
        // tgtd shares this namespace, so a sidecar that happens to be called
        // tgtd counts as a second instance.
        let mut instances: HashMap<&str, usize> =
            HashMap::from_iter(self.has_iscsi_disks().then_some(("tgtd", 1)));

        self.sidecar_services
            .iter()
            .flatten()
            .map(|cmd_str| {
                let binary = sidecar_binary_name(cmd_str);
                let instance = instances.entry(binary).or_default();
                *instance += 1;
                match *instance {
                    1 => (binary, log_file_name(binary)),
                    n => (binary, log_file_name(&format!("{binary}-{n}"))),
                }
            })
            .collect()
    }

    /// Every log file this machine produces on the host: one per sidecar
    /// service, plus tgtd's output and the iSCSI connection dump for
    /// iSCSI-backed disks.
    pub(crate) fn host_log_files(&self) -> Vec<HostLogFile> {
        let mut files = Vec::new();
        if self.has_iscsi_disks() {
            files.push(HostLogFile {
                name: "tgtd.log".to_owned(),
                description: "tgtd logs".to_owned(),
            });
            files.push(HostLogFile {
                name: ISCSI_LINGERING_CONNECTIONS_LOG.to_owned(),
                description: "iSCSI connections still open after VM exit".to_owned(),
            });
        }
        files.extend(
            self.sidecar_logs()
                .into_iter()
                .map(|(binary, name)| HostLogFile {
                    name,
                    description: format!("{binary} logs"),
                }),
        );
        files
    }
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
            vec!["bin", "--logs-dir", "/path/to/out"],
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
            logs_dir: Some("/tmp/whatever".into()),
            ..Default::default()
        };
        assert_eq!(
            args.get_vm_output_dirs(),
            HashSet::from(["/foo/bar".into()])
        );
    }

    #[test]
    fn test_sidecar_binary_name() {
        assert_eq!(sidecar_binary_name("tgtd"), "tgtd");
        assert_eq!(sidecar_binary_name("/path/to/tgtd x y z"), "tgtd");
        // Nothing to name the log after, but we must still return something
        assert_eq!(sidecar_binary_name(""), "unknown");
    }

    #[test]
    fn test_host_log_files() {
        let names = |machine: &MachineOpts| -> Vec<String> {
            machine
                .host_log_files()
                .into_iter()
                .map(|f| f.name)
                .collect()
        };

        let mut machine = MachineOpts::default();
        assert!(names(&machine).is_empty(), "No disks and no sidecars");

        machine.sidecar_services = vec![
            vec!["/path/to/foo --flag".to_string()],
            vec!["bar".to_string()],
        ];
        assert_eq!(names(&machine), vec!["foo.log", "bar.log"]);

        // tgtd runs on the host for iSCSI-backed disks, so its log and the
        // connection dump join the sidecar logs
        machine.disks = vec![QCow2DiskOpts::Iscsi(QCow2DiskIscsiOpts {
            common: QCow2DiskCommonOpts::default(),
            iscsi: IscsiOpts { ibft: false },
        })];
        assert_eq!(
            names(&machine),
            vec![
                "tgtd.log",
                "iscsi-lingering-connections.log",
                "foo.log",
                "bar.log"
            ]
        );

        // Running the same binary again must not clobber the first instance's
        // log, and must not disturb the name the first instance already has
        machine
            .sidecar_services
            .push(vec!["/other/bar --flag".to_string()]);
        assert_eq!(
            names(&machine),
            vec![
                "tgtd.log",
                "iscsi-lingering-connections.log",
                "foo.log",
                "bar.log",
                "bar-2.log"
            ]
        );

        // tgtd shares the namespace with sidecars
        machine.sidecar_services.push(vec!["tgtd".to_string()]);
        assert!(names(&machine).contains(&"tgtd-2.log".to_string()));
    }

    #[test]
    fn test_get_container_output_dirs() {
        let args = VMArgs {
            output_dirs: vec!["/foo/bar".into(), "/baz".into()],
            logs_dir: Some("/tmp/whatever".into()),
            ..Default::default()
        };
        assert_eq!(
            args.get_container_output_dirs(),
            HashSet::from(["/foo/bar".into(), "/baz".into(), "/tmp/whatever".into(),])
        );
    }
}
