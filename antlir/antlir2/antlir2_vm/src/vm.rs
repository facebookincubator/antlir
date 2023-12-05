/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::str::FromStr;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use thiserror::Error;
use tracing::debug;
use tracing::error;
use tracing::info;

use crate::disk::QCow2Disk;
use crate::disk::QCow2DiskBuilder;
use crate::disk::QCow2DiskError;
use crate::isolation::Platform;
use crate::net::VirtualNIC;
use crate::net::VirtualNICError;
use crate::pci::PCIBridge;
use crate::pci::PCIBridgeError;
use crate::pci::DEVICE_PER_BRIDGE;
use crate::runtime::get_runtime;
use crate::share::Share;
use crate::share::ShareError;
use crate::share::Shares;
use crate::ssh::GuestSSHCommand;
use crate::ssh::GuestSSHError;
use crate::tpm::TPMDevice;
use crate::tpm::TPMError;
use crate::types::CpuIsa;
use crate::types::MachineOpts;
use crate::types::ShareOpts;
use crate::types::TypeError;
use crate::types::VMArgs;
use crate::utils::log_command;

#[derive(Debug)]
pub(crate) struct VM<S: Share> {
    /// VM machine specification
    machine: MachineOpts,
    /// VM execution behavior
    args: VMArgs,
    /// List of PCI bridges
    pci_bridges: Vec<PCIBridge>,
    /// List of writable drives created for the VM. We need to hold the ownership
    /// to prevent the temporary disks from getting cleaned up prematuresly.
    disks: Vec<QCow2Disk>,
    /// All directories to be shared into the VM
    shares: Shares<S>,
    /// Virtual NICs to create and attach
    nics: Vec<VirtualNIC>,
    /// Directory to keep all ephemeral states
    state_dir: PathBuf,
    /// Handles to sidecar services
    sidecar_handles: Vec<JoinHandle<Result<ExitStatus>>>,
    /// TPM device
    tpm: Option<TPMDevice>,
}

#[derive(Error, Debug)]
pub(crate) enum VMError {
    #[error("Failed to create directory for VM states")]
    StateDirError(std::io::Error),
    #[error(transparent)]
    PCIBridgeError(#[from] PCIBridgeError),
    #[error(transparent)]
    DiskInitError(#[from] QCow2DiskError),
    #[error(transparent)]
    ShareInitError(#[from] ShareError),
    #[error(transparent)]
    NICInitError(#[from] VirtualNICError),
    #[error(transparent)]
    SSHCommandError(#[from] GuestSSHError),
    #[error(transparent)]
    TPMError(#[from] TPMError),
    #[error(transparent)]
    TypeError(#[from] TypeError),
    #[error("Failed to determine platform: {0}")]
    PlatformDetectionError(String),
    #[error("Failed to spawn qemu process: `{0}`")]
    QemuProcessError(std::io::Error),
    #[error("Failed to open output file: {path}: {err}")]
    FileOutputError { path: PathBuf, err: std::io::Error },
    #[error("Failed to start sidecar process: `{0}'")]
    SidecarError(std::io::Error),
    #[error("Failed to boot VM: {desc}: `{err}`")]
    BootError { desc: String, err: std::io::Error },
    #[error("VM terminated early unexpectedly: {0}")]
    EarlyTerminationError(ExitStatus),
    #[error("SSH Command exited with error: {0}")]
    SSHCommandResultError(ExitStatus),
    #[error("VM error after boot: `{0}`")]
    RunError(String),
    #[error("VM timed out")]
    TimeOutError,
}

type Result<T> = std::result::Result<T, VMError>;

impl<S: Share> VM<S> {
    /// Create a new VM along with its virtual resources
    pub(crate) fn new(machine: MachineOpts, args: VMArgs) -> Result<Self> {
        let state_dir = Self::create_state_dir()?;
        let pci_bridges = Self::create_pci_bridges(&machine)?;
        let disks = Self::create_disks(&machine, &pci_bridges, &state_dir)?;
        let shares = Self::create_shares(
            Self::get_all_shares_opts(&args.get_vm_output_dirs()),
            &state_dir,
            machine.mem_mib,
        )?;
        let nics = Self::create_nics(machine.num_nics)?;
        let tpm = match machine.use_tpm {
            true => Some(TPMDevice::new(&state_dir)?),
            false => None,
        };

        Ok(VM {
            machine,
            args,
            pci_bridges,
            disks,
            shares,
            nics,
            state_dir,
            sidecar_handles: vec![],
            tpm,
        })
    }

    /// Run the VM and wait for it to finish
    pub(crate) fn run(&mut self) -> Result<()> {
        self.sidecar_handles = self.spawn_sidecar_services();
        info!("Booting VM. It could take seconds to minutes...");
        let proc = self.spawn_vm()?;
        self.wait_for_vm(proc)?;
        Ok(())
    }

    /// Create a directory to store VM state. We rely on container for clean
    /// up to simplify resource tracking.
    fn create_state_dir() -> Result<PathBuf> {
        const STATE_DIR: &str = "/run/vm_state";
        fs::create_dir(STATE_DIR).map_err(VMError::StateDirError)?;
        Ok(PathBuf::from(STATE_DIR))
    }

    /// Create PCI bridges, enough for attaching all disks
    fn create_pci_bridges(opts: &MachineOpts) -> Result<Vec<PCIBridge>> {
        let num_bridges = (opts.disks.len() + DEVICE_PER_BRIDGE - 1) / DEVICE_PER_BRIDGE;
        (0..num_bridges)
            .map(|i| -> Result<PCIBridge> { Ok(PCIBridge::new(i, i + 1)?) })
            .collect()
    }

    /// Create all writable disks
    fn create_disks(
        opts: &MachineOpts,
        pci_bridges: &[PCIBridge],
        state_dir: &Path,
    ) -> Result<Vec<QCow2Disk>> {
        opts.disks
            .iter()
            .enumerate()
            .map(|(i, x)| {
                Ok(QCow2DiskBuilder::default()
                    .opts(x.clone())
                    .id(i)
                    .pci_bridge(pci_bridges[i / DEVICE_PER_BRIDGE].clone())
                    .state_dir(state_dir.to_path_buf())
                    .build()?)
            })
            .collect()
    }

    /// All platform paths needs to be mounted inside the VM as read-only shares.
    /// Collect them into ShareOpts along with others.
    fn get_all_shares_opts(output_dirs: &HashSet<PathBuf>) -> Vec<ShareOpts> {
        let mut shares: Vec<_> = Platform::get()
            .iter()
            .map(|path| ShareOpts {
                path: path.to_path_buf(),
                read_only: true,
                mount_tag: None,
            })
            .collect();
        let mut outputs: Vec<_> = output_dirs
            .iter()
            .map(|p| ShareOpts {
                path: p.to_path_buf(),
                read_only: false,
                mount_tag: None,
            })
            .collect();
        shares.append(&mut outputs);
        shares
    }

    /// Create all shares, start virtiofsd daemon and generate necessary unit files
    fn create_shares(shares: Vec<ShareOpts>, state_dir: &Path, mem_mb: usize) -> Result<Shares<S>> {
        let virtiofs_shares: Result<Vec<_>> = shares
            .into_iter()
            .enumerate()
            .map(|(i, opts)| -> Result<S> {
                let share = S::new(opts, i, state_dir.to_path_buf());
                share.setup()?;
                Ok(share)
            })
            .collect();
        let unit_files_dir = state_dir.join("mount_units");
        fs::create_dir(&unit_files_dir).map_err(VMError::StateDirError)?;
        let shares = Shares::new(virtiofs_shares?, mem_mb, unit_files_dir)?;
        shares.generate_unit_files()?;
        Ok(shares)
    }

    /// Create all virtual NICs
    fn create_nics(count: usize) -> Result<Vec<VirtualNIC>> {
        (0..count)
            .map(|x| -> Result<VirtualNIC> {
                let nic = VirtualNIC::new(x);
                nic.create_dev()?;
                Ok(nic)
            })
            .collect()
    }

    /// If timeout is specified, returns time until timeout, or TimeOutError
    /// if already timed out.
    fn time_left(&self, start_ts: Instant) -> Result<Duration> {
        match self.args.timeout_secs {
            Some(timeout) => {
                let elapsed = Instant::now()
                    .checked_duration_since(start_ts)
                    .unwrap_or(Duration::MAX);
                let left = Duration::from_secs(timeout.into()).saturating_sub(elapsed);
                if !left.is_zero() {
                    Ok(left)
                } else {
                    Err(VMError::TimeOutError)
                }
            }
            None => Ok(Duration::MAX),
        }
    }

    fn notify_file(&self) -> PathBuf {
        self.state_dir.join("vmtest_notify.sock")
    }

    /// The sidecar services will continue to run indefinitely until the outer
    /// container is torn down. Thus we just spawn them and forget.
    fn spawn_sidecar_services(&self) -> Vec<JoinHandle<Result<ExitStatus>>> {
        self.machine
            .sidecar_services
            .iter()
            // Buck passes all args as one string, so what we have is a list of
            // an inner list containing a single space-separated string as one
            // command to execute. This space splitting is not foolproof, but we
            // control sidecars so we can make it work.
            .flatten()
            .map(|args| {
                let args: Vec<&str> = args.split(' ').collect();
                let mut command = Command::new(args[0]);
                args.iter().skip(1).for_each(|c| {
                    command.arg(c);
                });
                thread::spawn(move || -> Result<ExitStatus> {
                    log_command(&mut command)
                        .status()
                        .map_err(VMError::SidecarError)
                })
            })
            .collect()
    }

    /// We assume the sidecar services are simple and fast, so we don't wait for
    /// their startup or have complicated scheme to assess their health. Just do
    /// a minimal check to ensure none have crashed immediately.
    fn check_sidecar_services(&mut self) -> Result<()> {
        if !self.sidecar_handles.iter().any(|x| x.is_finished()) {
            return Ok(());
        }

        // Print out the terimnated sidecar service(s) for debugging
        let results: Result<Vec<_>> = self
            .sidecar_handles
            .drain(..)
            .enumerate()
            .map(|(i, x)| -> Result<()> {
                if x.is_finished() {
                    let msg = format!("Sidecar service at index {} finished unexpectedly", i);
                    error!("{}", &msg);
                    let status = x.join().map_err(|e| {
                        VMError::SidecarError(std::io::Error::new(
                            ErrorKind::Other,
                            format!(
                                "Failed to join thread for sidecar service at index {}: {:?}",
                                i, e
                            ),
                        ))
                    })?;
                    error!("Exit status {:#?}", status);
                    return Err(VMError::SidecarError(std::io::Error::new(
                        ErrorKind::Other,
                        msg,
                    )));
                }
                Ok(())
            })
            .collect();
        results?;
        Ok(())
    }

    /// Figure out what we want to do with stdin/out/err for the VM process
    /// based on mode of operation.
    fn redirect_input_output(&self, mut command: Command) -> Result<Command> {
        // Leave stdin/out/err as is for console mode. The vm process we spawn
        // will be the same process we interact with in console mode.
        if !self.args.mode.console {
            // Disable stdin as input will come from elsewhere.
            command.stdin(Stdio::null());
            if let Some(path) = &self.args.console_output_file {
                // Redirect stdout/err to a file if specified.
                let map_err = |err| VMError::FileOutputError {
                    path: path.to_owned(),
                    err,
                };
                let file = File::create(path).map_err(map_err)?;
                command.stdout(file.try_clone().map_err(map_err)?);
                command.stderr(file.try_clone().map_err(map_err)?);
            } else {
                // Disable stdout/err if we neither need it on screen or in file.
                command.stdout(Stdio::null());
                command.stderr(Stdio::null());
            }
        }
        Ok(command)
    }

    /// Spawn qemu-system process. It won't immediately start running until we connect
    /// to the notify socket.
    fn spawn_vm(&self) -> Result<Child> {
        let mut args = self.common_qemu_args()?;
        args.extend(self.non_disk_boot_qemu_args());
        args.extend(self.pci_bridge_qemu_args());
        args.extend(self.disk_qemu_args());
        args.extend(self.share_qemu_args());
        args.extend(self.nic_qemu_args());
        if let Some(tpm) = &self.tpm {
            args.extend(tpm.qemu_args());
        }
        let mut command = Command::new(&get_runtime().qemu_system);
        command = self.redirect_input_output(command)?;
        let command = command.args(&args);

        log_command(command)
            .spawn()
            .map_err(VMError::QemuProcessError)
    }

    /// Closing the notify socket will result in VM's termination. If VM
    /// terminates on its own, the socket will be closed. So we poll the notify
    /// socket until timeout. If we are waiting for a thread, complete early if
    /// the thread exits.
    fn wait_for_timeout<T>(
        &self,
        mut socket: UnixStream,
        start_ts: Instant,
        thread_handle: Option<JoinHandle<T>>,
    ) -> Result<Option<T>> {
        // Poll until either socket close or timeout. The buffer size is arbitrary,
        // because we don't expect any data.
        let mut buf = [0; 8];
        socket
            .set_nonblocking(true)
            .map_err(|err| VMError::BootError {
                desc: "Failed to set non-blocking socket option".into(),
                err,
            })?;
        let mut result = None;
        while !self.time_left(start_ts)?.is_zero() {
            if let Some(ref handle) = thread_handle {
                if handle.is_finished() {
                    result = Some(
                        thread_handle
                            .expect("Handle must exist here")
                            .join()
                            .map_err(|e| {
                                VMError::RunError(format!("SSH command thread panic'ed: {:?}", e))
                            })?,
                    );
                    break;
                }
            }
            match socket.read(&mut buf) {
                Ok(0) => {
                    debug!("Notify socket closed. VM exited");
                    break;
                }
                Ok(_) => debug!("Received unexpected data from VM notify socket"),
                Err(_) => thread::sleep(Duration::from_secs(1)),
            }
        }
        // Finishing the command is a success for this function. Let caller
        // decide how to interpret the results.
        Ok(result)
    }

    /// Execute ssh command and wait for timeout specified for the VM.
    fn run_cmd_and_wait(
        &self,
        mut cmd: Command,
        socket: UnixStream,
        start_ts: Instant,
    ) -> Result<ExitStatus> {
        // Spawn command in a separate thread so that we can enforce timeout.
        let handle = thread::spawn(move || {
            log_command(&mut cmd).status().map_err(|err| {
                VMError::RunError(format!("Failed to run command `{:?}`: {}", cmd, err))
            })
        });
        let status = self.wait_for_timeout(socket, start_ts, Some(handle))?;
        match status {
            Some(s) => Ok(s?),
            None => Err(VMError::RunError(
                "Command didn't return before VM is terminated".into(),
            )),
        }
    }

    /// We control VM process through sockets. If VM process exited for any reason
    /// before socket connection is established, it's an error. Detect such early
    /// failure by polling process status.
    fn try_wait_vm_proc(&self, child: &mut Child) -> Result<()> {
        match child.try_wait() {
            Ok(Some(status)) => Err(VMError::EarlyTerminationError(status)),
            Ok(None) => Ok(()),
            Err(err) => Err(VMError::BootError {
                desc: "Error attempting to wait for VM process".into(),
                err,
            }),
        }
    }

    /// Connect to the notify socket, wait for boot ready message and wait for the VM
    /// to terminate. If time out is specified, this function will return error
    /// upon timing out.
    fn wait_for_vm(&mut self, mut vm_proc: Child) -> Result<()> {
        let start_ts = Instant::now();

        // Wait for notify file to be created by qemu
        debug!("Waiting for notify file to be created");
        while !self.time_left(start_ts)?.is_zero() {
            match self.notify_file().try_exists() {
                Ok(true) => break,
                Ok(false) => {
                    self.try_wait_vm_proc(&mut vm_proc)?;
                    thread::sleep(Duration::from_millis(100));
                }
                Err(err) => {
                    return Err(VMError::BootError {
                        desc: "Unable to access notify file".into(),
                        err,
                    });
                }
            }
        }

        // Connect to the notify socket. This starts the boot process.
        self.check_sidecar_services()?;
        if !self.args.mode.console {
            if let Some(console_file) = &self.args.console_output_file {
                info!(
                    "Note: console output is redirected to {}",
                    console_file.display()
                );
            }
        }
        let socket = UnixStream::connect(self.notify_file()).map_err(|err| VMError::BootError {
            desc: "Failed to connect to notify socket".into(),
            err,
        })?;

        // Spawn container shell immediately if requested. VM is probably not
        // booting or one wouldn't be debugging this. There is also nothing to
        // do once the container shell closes.
        if self.args.mode.container {
            let mut cmd = Command::new("/bin/bash");
            cmd.arg("-l");
            self.run_cmd_and_wait(cmd, socket, start_ts)?;
            return Ok(());
        }

        // Wait for boot notify message. We expect "READY" message once VM boots
        debug!("Waiting for boot notify message");
        if self.args.timeout_secs.is_some() {
            socket
                .set_read_timeout(Some(self.time_left(start_ts)?))
                .map_err(|err| VMError::BootError {
                    desc: "Failed to set notify socket read timeout".into(),
                    err,
                })?;
        }
        let mut response = String::new();
        let mut f = BufReader::new(socket);
        f.read_line(&mut response)
            .map_err(|err| VMError::BootError {
                desc: "Failed to read notify socket".into(),
                err,
            })?;
        info!(
            "Received boot event {} after {} seconds",
            response.trim(),
            start_ts.elapsed().as_secs_f32()
        );

        // VM booted
        self.check_sidecar_services()?;
        let mut exit_status = None;
        if self.args.mode.console {
            // Just wait for the human that's trying to debug with console
            self.wait_for_timeout::<()>(f.into_inner(), start_ts, None)?;
        } else if !self.args.mode.container {
            let mut ssh_cmd = GuestSSHCommand::new()?.ssh_cmd();
            if self.args.mode.command.is_none() {
                // Force pseudo-terminal allocation for interactive use case. Or
                // ssh hang instead because we add a bash command below.
                ssh_cmd.arg("-t");
            }
            self.args.command_envs.iter().for_each(|kv| {
                ssh_cmd.arg(kv.to_os_string());
            });
            if let Some(command) = &self.args.mode.command {
                ssh_cmd.args(command);
            } else {
                ssh_cmd.args(["/bin/bash", "-l"]);
            }
            exit_status = Some(self.run_cmd_and_wait(ssh_cmd, f.into_inner(), start_ts)?);
        }
        info!("VM executed for {} seconds", start_ts.elapsed().as_secs());

        // We care about exit code only if we are running a command
        if let Some(status) = exit_status {
            if self.args.mode.command.is_some() && !status.success() {
                return Err(VMError::SSHCommandResultError(status));
            }
        }
        Ok(())
    }

    // Query current arch that's executing this binary.
    fn current_arch(&self) -> Result<CpuIsa> {
        let uname = Command::new("uname")
            .arg("-m")
            .output()
            .map_err(|e| VMError::PlatformDetectionError(format!("uname command failed: {}", e)))?;
        let output = String::from_utf8(uname.stdout).map_err(|e| {
            VMError::PlatformDetectionError(format!("Failed to parse uname output: {}", e))
        })?;
        let arch = output.trim();
        debug!("Detected current execution platform: {}", arch);
        Ok(CpuIsa::from_str(arch)?)
    }

    // Some args depending on whether the execution platform is same as the
    // platform being emulated.
    fn arch_emulation_args(&self, current_arch: CpuIsa) -> Vec<OsString> {
        let args = if current_arch == self.machine.arch {
            vec!["-cpu", "host", "-enable-kvm"]
        } else {
            vec!["-cpu", "max"]
        };
        args.into_iter().map(|x| x.into()).collect()
    }

    fn common_qemu_args(&self) -> Result<Vec<OsString>> {
        let mut args = vec![];
        args.append(
            &mut [
                // General
                "-no-reboot",
                "-display",
                "none",
            ]
            .iter()
            .map(|x| x.into())
            .collect(),
        );

        let mut serial = vec![];
        (0..self.machine.serial_index).for_each(|_| {
            serial.push("-serial");
            serial.push("null");
        });
        serial.append(&mut vec!["-serial", "mon:stdio"]);
        args.append(&mut serial.into_iter().map(|x| x.into()).collect());

        args.append(
            &mut [
                // Basic machine info
                "-machine",
                match self.machine.arch {
                    CpuIsa::AARCH64 => "virt",
                    CpuIsa::X86_64 => "pc",
                },
                "-smp",
                &self.machine.cpus.to_string(),
                "-m",
                &format!("{}M", self.machine.mem_mib),
                // Common devices
                "-object",
                "rng-random,filename=/dev/urandom,id=rng0",
                "-device",
                "virtio-rng-pci,rng=rng0",
                "-device",
                "virtio-serial",
                // socket/serial device pair (for communicating with VM)
                "-chardev",
                &format!(
                    "socket,path={},id=notify,server=on",
                    self.notify_file().to_str().expect("Invalid file name")
                ),
                "-device",
                "virtserialport,chardev=notify,name=notify-host",
                // firmware
                "-drive",
                &format!(
                    "if=pflash,format=raw,unit=0,file={},readonly=on",
                    get_runtime().firmware,
                ),
                // ROM
                "-L",
                &get_runtime().roms_dir,
            ]
            .iter()
            .map(|x| x.into())
            .collect(),
        );
        args.extend(self.arch_emulation_args(self.current_arch()?));
        Ok(args)
    }

    fn pci_bridge_qemu_args(&self) -> Vec<OsString> {
        self.pci_bridges
            .iter()
            .flat_map(|x| x.qemu_args())
            .collect()
    }

    fn disk_qemu_args(&self) -> Vec<OsString> {
        self.disks.iter().flat_map(|x| x.qemu_args()).collect()
    }

    fn non_disk_boot_qemu_args(&self) -> Vec<OsString> {
        match &self.machine.non_disk_boot_opts {
            Some(opts) => {
                let mut args: Vec<_> = [
                    "-initrd",
                    &opts.initrd,
                    // kernel
                    "-kernel",
                    &opts.kernel,
                ]
                .iter()
                .map(|x| x.into())
                .collect();
                if !opts.append.is_empty() {
                    args.push("-append".into());
                    args.push(opts.append.clone().into());
                }
                args
            }
            None => vec![],
        }
    }

    fn share_qemu_args(&self) -> Vec<OsString> {
        self.shares.qemu_args()
    }

    fn nic_qemu_args(&self) -> Vec<OsString> {
        self.nics.iter().flat_map(|x| x.qemu_args()).collect()
    }
}

#[cfg(test)]
mod test {
    use std::net::Shutdown;
    use std::thread;

    use super::*;
    use crate::runtime::set_runtime;
    use crate::share::VirtiofsShare;
    use crate::types::NonDiskBootOpts;
    use crate::types::RuntimeOpts;
    use crate::types::VMArgs;
    use crate::utils::qemu_args_to_string;

    fn get_vm_no_disk() -> VM<VirtiofsShare> {
        let machine = MachineOpts {
            cpus: 1,
            mem_mib: 1024,
            num_nics: 1,
            ..Default::default()
        };
        let args = VMArgs::default();
        let share_opts = ShareOpts {
            path: PathBuf::from("/path"),
            read_only: true,
            mount_tag: None,
        };
        let share = VirtiofsShare::new(share_opts, 1, PathBuf::from("/state"));
        VM {
            machine,
            args,
            pci_bridges: vec![],
            disks: vec![],
            shares: Shares::new(vec![share], 1024, PathBuf::from("/state/units"))
                .expect("Failed to create Shares"),
            nics: vec![VirtualNIC::new(0)],
            state_dir: PathBuf::from("/test/path"),
            sidecar_handles: vec![],
            tpm: None,
        }
    }

    fn set_bogus_runtime() {
        set_runtime(RuntimeOpts {
            qemu_system: "qemu-system".to_string(),
            qemu_img: "qemu-img".to_string(),
            firmware: "edk2-arch-code.fd".to_string(),
            roms_dir: "roms".to_string(),
            swtpm: "swtpm".to_string(),
        })
        .expect("Failed to set fake runtime");
    }

    #[test]
    fn test_arch_emulation_args() {
        let mut vm = get_vm_no_disk();
        vm.machine.arch = CpuIsa::AARCH64;
        assert_eq!(
            vm.arch_emulation_args(CpuIsa::AARCH64),
            vec!["-cpu", "host", "-enable-kvm"],
        );
        assert_eq!(vm.arch_emulation_args(CpuIsa::X86_64), vec!["-cpu", "max"]);

        vm.machine.arch = CpuIsa::X86_64;
        assert_eq!(vm.arch_emulation_args(CpuIsa::AARCH64), vec!["-cpu", "max"]);
        assert_eq!(
            vm.arch_emulation_args(CpuIsa::X86_64),
            vec!["-cpu", "host", "-enable-kvm"],
        );
    }

    #[test]
    fn test_common_qemu_args() {
        let mut vm = get_vm_no_disk();
        set_bogus_runtime();
        let common_args =
            qemu_args_to_string(&vm.common_qemu_args().expect("Failed to build qemu args"));
        assert!(common_args.contains("-cpu "));
        assert!(common_args.contains("-smp 1"));
        assert!(common_args.contains("-m 1024M"));
        assert!(common_args.contains(&format!(
            "-chardev socket,path={}/vmtest_notify.sock,id=notify,server=on",
            vm.state_dir.to_str().expect("Invalid tempdir path"),
        )));
        assert!(
            common_args.contains("if=pflash,format=raw,unit=0,file=edk2-arch-code.fd,readonly=on")
        );
        assert!(common_args.contains("-L roms"));

        vm.machine.serial_index = 2;
        let common_args =
            qemu_args_to_string(&vm.common_qemu_args().expect("Failed to build qemu args"));
        assert!(common_args.contains("none -serial null -serial null -serial mon:stdio"));
    }

    #[test]
    fn test_time_left() {
        let mut vm = get_vm_no_disk();

        // no timeout
        assert_eq!(
            vm.time_left(Instant::now()).expect("Unexpected timeout"),
            Duration::MAX
        );

        // with timeout
        vm.args.timeout_secs = Some(60);
        let start_ts = Instant::now();
        assert!(vm.time_left(start_ts).expect("Unexpected timeout") > Duration::from_secs(1));
        vm.args.timeout_secs = Some(1);
        thread::sleep(Duration::from_secs(1));
        assert!(vm.time_left(start_ts).is_err());
    }

    #[test]
    fn test_non_boot_qemu_args() {
        let mut vm = get_vm_no_disk();
        assert_eq!(vm.non_disk_boot_qemu_args(), Vec::<OsString>::new());

        vm.machine.non_disk_boot_opts = Some(NonDiskBootOpts {
            initrd: "initrd".to_string(),
            kernel: "kernel".to_string(),
            append: "whatever".to_string(),
        });
        let args = qemu_args_to_string(&vm.non_disk_boot_qemu_args());
        assert!(args.contains("-initrd initrd"));
        assert!(args.contains("-kernel kernel"));
        assert!(args.contains("-append whatever"));
    }

    #[test]
    fn test_wait_for_timeout_without_command() {
        // Terminate after timeout
        let mut vm = get_vm_no_disk();
        vm.args.timeout_secs = Some(3);
        let (_send, recv) = UnixStream::pair().expect("Failed to create sockets");
        let start_ts = Instant::now();
        let handle = thread::spawn(move || {
            assert!(vm.wait_for_timeout::<()>(recv, start_ts, None).is_err());
        });
        thread::sleep(Duration::from_secs(1));
        assert!(!handle.is_finished());
        handle.join().expect("Test thread panic'ed");
        let elapsed = Instant::now()
            .checked_duration_since(start_ts)
            .expect("Invalid duration");
        assert!(elapsed > Duration::from_secs(3));

        // Terminate before timeout due to closed socket
        let mut vm = get_vm_no_disk();
        vm.args.timeout_secs = Some(10);
        let (send, recv) = UnixStream::pair().expect("Failed to create sockets");
        let start_ts = Instant::now();
        let handle = thread::spawn(move || {
            assert!(vm.wait_for_timeout::<()>(recv, start_ts, None).is_ok());
        });
        thread::sleep(Duration::from_secs(1));
        assert!(!handle.is_finished());
        send.shutdown(Shutdown::Both)
            .expect("Failed to shutdown sender");
        handle.join().expect("Test thread panic'ed");
        let elapsed = Instant::now()
            .checked_duration_since(start_ts)
            .expect("Invalid duration");
        assert!(elapsed < Duration::from_secs(10));

        // Without timeout
        let mut vm = get_vm_no_disk();
        vm.args.timeout_secs = None;
        let (send, recv) = UnixStream::pair().expect("Failed to create sockets");
        let handle = thread::spawn(move || {
            assert!(
                vm.wait_for_timeout::<()>(recv, Instant::now(), None)
                    .is_ok()
            );
        });
        thread::sleep(Duration::from_secs(1));
        assert!(!handle.is_finished());
        send.shutdown(Shutdown::Both)
            .expect("Failed to shutdown sender");
        handle.join().expect("Test thread panic'ed");
    }

    #[test]
    fn test_wait_for_timeout_with_command() {
        // Command finished before timeout.
        let mut vm = get_vm_no_disk();
        vm.args.timeout_secs = Some(10);
        let start_ts = Instant::now();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_secs(3));
        });
        let (send, recv) = UnixStream::pair().expect("Failed to create sockets");
        assert!(
            vm.wait_for_timeout::<()>(recv, start_ts, Some(handle))
                .is_ok()
        );
        let elapsed = Instant::now()
            .checked_duration_since(start_ts)
            .expect("Invalid duration");
        assert!(elapsed < Duration::from_secs(10));
        assert!(elapsed > Duration::from_secs(3));
        send.shutdown(Shutdown::Both)
            .expect("Failed to shutdown sender");

        // Command exceeded timeout.
        let mut vm = get_vm_no_disk();
        vm.args.timeout_secs = Some(3);
        let start_ts = Instant::now();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_secs(5));
        });
        let (send, recv) = UnixStream::pair().expect("Failed to create sockets");
        assert!(
            vm.wait_for_timeout::<()>(recv, start_ts, Some(handle))
                .is_err()
        );
        assert!(elapsed > Duration::from_secs(3));
        assert!(elapsed < Duration::from_secs(5));
        send.shutdown(Shutdown::Both)
            .expect("Failed to shutdown sender");
    }

    #[test]
    fn test_run_cmd_and_wait() {
        let mut vm = get_vm_no_disk();
        vm.args.timeout_secs = Some(1);

        // timeout
        let (_send, recv) = UnixStream::pair().expect("Failed to create sockets");
        let mut command = Command::new("/usr/bin/sleep");
        command.arg("5");
        assert!(vm.run_cmd_and_wait(command, recv, Instant::now()).is_err());

        // reset timeout for all tests below that shouldn't timeout
        vm.args.timeout_secs = Some(5);

        // command completes with success
        let (_send, recv) = UnixStream::pair().expect("Failed to create sockets");
        let command = Command::new("/usr/bin/ls");
        let result = vm.run_cmd_and_wait(command, recv, Instant::now());
        println!("{:?}", result);
        assert!(result.is_ok());
        assert!(result.expect("Already checked").success());

        // command completes with non-zero exit code
        let (_send, recv) = UnixStream::pair().expect("Failed to create sockets");
        let mut command = Command::new("/usr/bin/ls");
        command.arg("non_existent_file");
        let result = vm.run_cmd_and_wait(command, recv, Instant::now());
        println!("{:?}", result);
        assert!(result.is_ok());
        assert!(!result.expect("Already checked").success());
    }

    #[test]
    fn test_try_wait_vm_proc() {
        let vm = get_vm_no_disk();
        let mut child = Command::new("sleep")
            .arg("1")
            .spawn()
            .expect("Failed to spawn test process");
        assert!(vm.try_wait_vm_proc(&mut child).is_ok());
        Command::new("sleep")
            .arg("1")
            .status()
            .expect("Failed to finish second sleep");
        assert!(vm.try_wait_vm_proc(&mut child).is_err());
    }

    #[test]
    fn test_get_all_shares_opts() {
        Platform::set().expect("Failed to query platform");
        let outputs = HashSet::from([PathBuf::from("/path")]);
        let opt = ShareOpts {
            path: PathBuf::from("/path"),
            read_only: false,
            mount_tag: None,
        };
        let all_opts = VM::<VirtiofsShare>::get_all_shares_opts(&outputs);
        assert!(all_opts.contains(&opt));
    }

    #[test]
    fn test_sidecar_services_happy() {
        let mut vm = get_vm_no_disk();
        vm.machine.sidecar_services = vec![
            vec!["sleep".to_string(), "3".to_string()],
            vec!["sleep".to_string(), "5".to_string()],
        ];
        vm.sidecar_handles = vm.spawn_sidecar_services();
        assert!(vm.check_sidecar_services().is_ok());
    }

    #[test]
    fn test_sidecar_services_early_finish() {
        let mut vm = get_vm_no_disk();
        vm.machine.sidecar_services = vec![vec!["command_does_not_exist".to_string()]];
        vm.sidecar_handles = vm.spawn_sidecar_services();
        thread::sleep(Duration::from_secs(1));
        assert!(vm.check_sidecar_services().is_err());
    }
}
