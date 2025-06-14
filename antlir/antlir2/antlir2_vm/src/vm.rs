/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
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
use tracing::warn;
use uuid::Uuid;

use crate::disk::QCow2DiskError;
use crate::disk::QCow2Disks;
use crate::isolation::IsolationError;
use crate::isolation::Platform;
use crate::net::VirtualNICError;
use crate::net::VirtualNICs;
use crate::pci::PCIBridgeError;
use crate::pci::PCIBridges;
use crate::share::Share;
use crate::share::ShareError;
use crate::share::Shares;
use crate::ssh::GuestSSHCommand;
use crate::ssh::GuestSSHError;
use crate::tpm::TPMDevice;
use crate::tpm::TPMError;
use crate::types::CpuIsa;
use crate::types::MachineOpts;
use crate::types::QemuDevice;
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
    pci_bridges: PCIBridges,
    /// List of writable drives created for the VM. We need to hold the ownership
    /// to prevent the temporary disks from getting cleaned up prematuresly.
    disks: QCow2Disks,
    /// All directories to be shared into the VM
    shares: Shares<S>,
    /// Virtual NICs to create and attach
    nics: VirtualNICs,
    /// Directory to keep all ephemeral states
    state_dir: PathBuf,
    /// Handles to sidecar services
    sidecar_handles: Vec<JoinHandle<Result<ExitStatus>>>,
    /// TPM device
    tpm: Option<TPMDevice>,
    /// Uuid for this VM. Randomly generated to aid debugging when multiple VMs are running
    identifier: String,
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
    #[error("Failed to clean up: {desc}: `{err}`")]
    CleanupError { desc: String, err: std::io::Error },
    #[error(transparent)]
    Isolation(#[from] IsolationError),
}

type Result<T> = std::result::Result<T, VMError>;

impl<S: Share> VM<S> {
    /// Create a new VM along with its virtual resources
    pub(crate) fn new(machine: MachineOpts, args: VMArgs) -> Result<Self> {
        let state_dir = Self::create_state_dir()?;
        let pci_bridges = PCIBridges::new(machine.disks.len())?;
        let disks = QCow2Disks::new(&machine.disks, &pci_bridges, &state_dir)?;
        let shares = Self::create_shares(
            Self::get_all_shares_opts(&args.get_vm_output_dirs()),
            &state_dir,
            machine.mem_mib,
        )?;
        let mut nics = VirtualNICs::new(machine.num_nics, machine.max_combined_channels)?;
        if nics.len() > 0 {
            if let Err(e) = nics[0].try_dump_file(args.eth0_output_file.clone()) {
                let err = format!("Failed to set eth0 dump file: {:?}", e);
                warn!(err);
                // Leave a hint that we could not set the dump file by writting a textual error in the .pcap file.
                // This will generate a corrupted .pcap file that an operator can look into to debug and understand what went wrong.
                if let Some(filename) = args.eth0_output_file.as_ref() {
                    // If any part of this fail, we don't want to fail the VM creation.
                    let _ =
                        fs::File::create(filename).and_then(|mut f| f.write_all(err.as_bytes()));
                }
            }
        }
        let tpm = match machine.use_tpm {
            true => Some(TPMDevice::new(&state_dir)?),
            false => None,
        };
        let identifier = Uuid::new_v4().to_string();

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
            identifier,
        })
    }

    /// Run the VM and wait for it to finish
    pub(crate) fn run(&mut self) -> Result<()> {
        let start_ts = Instant::now();
        self.sidecar_handles = self.spawn_sidecar_services();
        if self.args.first_boot_command.is_some() {
            info!("Booting VM for first boot command. It could take seconds to minutes...");
            let proc = self.spawn_vm()?;
            let ssh_first_boot_cmd = self.ssh_first_boot_command()?;
            self.wait_for_vm(proc, ssh_first_boot_cmd, true, start_ts)?;
            thread::sleep(Duration::from_secs(1));
        }
        info!("Booting VM. It could take seconds to minutes...");
        let proc = self.spawn_vm()?;
        let ssh_cmd = self.ssh_command()?;
        self.wait_for_vm(proc, ssh_cmd, false, start_ts)?;
        Ok(())
    }

    /// Create a directory to store VM state. We rely on container for clean
    /// up to simplify resource tracking.
    fn create_state_dir() -> Result<PathBuf> {
        const STATE_DIR: &str = "/run/vm_state";
        fs::create_dir(STATE_DIR).map_err(VMError::StateDirError)?;
        Ok(PathBuf::from(STATE_DIR))
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
                Ok(share)
            })
            .collect();
        let unit_files_dir = state_dir.join("mount_units");
        fs::create_dir(&unit_files_dir).map_err(VMError::StateDirError)?;
        let shares = Shares::new(virtiofs_shares?, mem_mb, unit_files_dir)?;
        shares.generate_unit_files()?;
        Ok(shares)
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
        self.state_dir
            .join(format!("vmtest_notify-{}.sock", self.identifier))
    }

    fn ssh_command(&self) -> Result<Command> {
        let mut ssh_cmd = GuestSSHCommand::new()?.ssh_cmd();
        if self.args.mode.command.is_none() {
            // Force pseudo-terminal allocation for interactive use case. Or
            // ssh hang instead because we add a bash command below.
            ssh_cmd.arg("-t");
        }
        let mut cd_cmd = OsString::from("cd ");
        cd_cmd.push(Platform::repo_root()?);
        cd_cmd.push(";");
        ssh_cmd.arg(cd_cmd);
        ssh_cmd.arg("env");
        ssh_cmd.arg("--");
        self.args.command_envs.iter().for_each(|kv| {
            ssh_cmd.arg(kv.to_os_string_for_env());
        });
        if let Some(command) = &self.args.mode.command {
            ssh_cmd.args(command);
        } else {
            ssh_cmd.args(["/bin/bash", "-l"]);
        }
        Ok(ssh_cmd)
    }

    fn ssh_first_boot_command(&self) -> Result<Command> {
        let mut ssh_cmd = GuestSSHCommand::new()?.ssh_cmd();
        ssh_cmd.arg("env");
        self.args.command_envs.iter().for_each(|kv| {
            ssh_cmd.arg(kv.to_os_string_for_env());
        });

        if let Some(command) = &self.args.first_boot_command {
            // Try to canonicalize the firstboot command to an absolute path
            // which will be valid in the vm
            ssh_cmd.arg(std::fs::canonicalize(command).unwrap_or_else(|_| command.into()));
        }

        info!("First boot command: {:?}", ssh_cmd);
        Ok(ssh_cmd)
    }

    fn ssh_poweroff_command(&self) -> Result<Command> {
        let mut ssh_cmd = GuestSSHCommand::new()?.ssh_cmd();
        ssh_cmd.arg("nohup shutdown 1 &> /dev/null &disown");
        Ok(ssh_cmd)
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
                // Use append to not lose stdout/err from previous runs.
                let file = OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(path)
                    .map_err(map_err)?;
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
        // Start virtiofsd daemons now that we are about to launch QEMU
        self.shares.start_shares()?;

        let mut args = self.common_qemu_args()?;
        args.extend(self.non_disk_boot_qemu_args());
        args.extend(self.pci_bridges.qemu_args());
        args.extend(self.disks.qemu_args());
        args.extend(self.shares.qemu_args());
        args.extend(self.nics.qemu_args());
        if let Some(tpm) = &self.tpm {
            args.extend(tpm.qemu_args());
        }

        let mut command = Command::new(match self.machine.arch {
            CpuIsa::AARCH64 => "qemu-system-aarch64",
            CpuIsa::X86_64 => "qemu-system-x86_64",
        });
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
        mut socket: &UnixStream,
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
                Ok(_read) => debug!("Received unexpected data from VM notify socket: {buf:?}"),
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
        socket: &UnixStream,
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

    /// We no longer expect / need the VM to be running. Let's clean up the process
    /// within the allowed timeout window.
    fn cleanup_vm(
        &mut self,
        mut vm_proc: Child,
        socket: &UnixStream,
        cleanup_needed: bool,
        start_ts: Instant,
    ) -> Result<()> {
        if !cleanup_needed {
            // Do not clean up the VM resources if we are exiting in any case.
            // Rely on the container / process exit instead. This reduces the
            // risk of reporting errors after successful workload runs.
            return Ok(());
        }
        // If the VM is still running, shut it down
        if vm_proc
            .try_wait()
            .map_err(|err| VMError::CleanupError {
                desc: "Error attempting to wait on the VM process".into(),
                err,
            })?
            .is_none()
        {
            let poweroff = self.ssh_poweroff_command()?;
            self.run_cmd_and_wait(poweroff, socket, start_ts)?;

            while !self.time_left(start_ts)?.is_zero() {
                match vm_proc.try_wait() {
                    Ok(Some(_)) => {
                        break;
                    }
                    Ok(None) => {
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(err) => {
                        return Err(VMError::CleanupError {
                            desc: "Error attempting to wait on the VM process".into(),
                            err,
                        });
                    }
                }
            }
        }

        // We are done with the socket. Close it.
        socket
            .shutdown(Shutdown::Both)
            .map_err(|err| VMError::CleanupError {
                desc: "Failed to shutdown socket".into(),
                err,
            })?;

        // Remove the notify file if it exists
        match self.notify_file().try_exists() {
            Ok(false) => {} // do nothing,
            Ok(true) => {
                // delete the file
                match std::fs::remove_file(self.notify_file()) {
                    Ok(_) => {}
                    Err(err) => {
                        return Err(VMError::CleanupError {
                            desc: format!(
                                "Unable to remove notify file {}",
                                self.notify_file().to_str().expect("Invalid file name")
                            ),
                            err,
                        });
                    }
                }
            }
            Err(err) => {
                return Err(VMError::CleanupError {
                    desc: format!(
                        "Unable to access notify file {}",
                        self.notify_file().to_str().expect("Invalid file name")
                    ),
                    err,
                });
            }
        }
        Ok(())
    }

    /// Connect to the notify socket, wait for boot ready message and wait for the VM
    /// to terminate. If time out is specified, this function will return error
    /// upon timing out.
    fn wait_for_vm(
        &mut self,
        mut vm_proc: Child,
        ssh_cmd: Command,
        cleanup_needed: bool,
        start_ts: Instant,
    ) -> Result<()> {
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
            self.run_cmd_and_wait(cmd, &socket, start_ts)?;
            self.cleanup_vm(vm_proc, &socket, cleanup_needed, start_ts)?;
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
        let desc = "Failed to read boot event from the notify socket. This
        indicates the VM failed to boot to default target. Please check the
        console log for further analysis"
            .into();
        f.read_line(&mut response)
            .map_err(|err| VMError::BootError { desc, err })?;
        info!(
            "Received boot event {} after {} seconds",
            response.trim(),
            start_ts.elapsed().as_secs_f32()
        );
        let socket = f.into_inner();

        // VM booted
        self.check_sidecar_services()?;
        let mut exit_status = None;
        if self.args.mode.console {
            // Just wait for the human that's trying to debug with console
            self.wait_for_timeout::<()>(&socket, start_ts, None)?;
        } else if !self.args.mode.container {
            exit_status = Some(self.run_cmd_and_wait(ssh_cmd, &socket, start_ts)?);
        }
        info!("VM executed for {} seconds", start_ts.elapsed().as_secs());

        // We care about exit code only if we are running a command
        if let Some(status) = exit_status {
            if self.args.mode.command.is_some() && !status.success() {
                return Err(VMError::SSHCommandResultError(status));
            }
        }
        self.cleanup_vm(vm_proc, &socket, cleanup_needed, start_ts)?;
        Ok(())
    }

    // Query current arch that's executing this binary.
    fn current_arch(&self) -> CpuIsa {
        CpuIsa::from_str(std::env::consts::ARCH).expect("unknown cpu architecture")
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
                    match self.machine.arch {
                        CpuIsa::AARCH64 => "/usr/share/edk2/aarch64/QEMU_EFI.fd",
                        CpuIsa::X86_64 => "/usr/share/edk2/ovmf/OVMF_CODE.fd",
                    }
                ),
            ]
            .iter()
            .map(|x| x.into())
            .collect(),
        );
        args.extend(self.arch_emulation_args(self.current_arch()));
        Ok(args)
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
}

#[cfg(test)]
mod test {
    use std::net::Shutdown;
    use std::thread;

    use super::*;
    use crate::share::VirtiofsShare;
    use crate::types::MountPlatformDecision;
    use crate::types::NonDiskBootOpts;
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
        let pci_bridges = PCIBridges::new(0).expect("Failed to create PCIBridges");
        let disks = QCow2Disks::new(&[], &pci_bridges, Path::new("/state/units"))
            .expect("Failed to create disks");
        let nics = VirtualNICs::new(0, 0).expect("Failed to create NICs");
        VM {
            machine,
            args,
            pci_bridges,
            disks,
            shares: Shares::new(vec![share], 1024, PathBuf::from("/state/units"))
                .expect("Failed to create Shares"),
            nics,
            state_dir: PathBuf::from("/test/path"),
            sidecar_handles: vec![],
            tpm: None,
            identifier: "one".to_string(),
        }
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
        let common_args =
            qemu_args_to_string(&vm.common_qemu_args().expect("Failed to build qemu args"));
        assert!(common_args.contains("-cpu "));
        assert!(common_args.contains("-smp 1"));
        assert!(common_args.contains("-m 1024M"));
        assert!(common_args.contains(&format!(
            "-chardev socket,path={}/vmtest_notify-one.sock,id=notify,server=on",
            vm.state_dir.to_str().expect("Invalid tempdir path"),
        )));
        assert!(common_args.contains(
            "if=pflash,format=raw,unit=0,file=/usr/share/edk2/ovmf/OVMF_CODE.fd,readonly=on"
        ));

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
            assert!(vm.wait_for_timeout::<()>(&recv, start_ts, None).is_err());
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
            assert!(vm.wait_for_timeout::<()>(&recv, start_ts, None).is_ok());
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
                vm.wait_for_timeout::<()>(&recv, Instant::now(), None)
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
            vm.wait_for_timeout::<()>(&recv, start_ts, Some(handle))
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
            vm.wait_for_timeout::<()>(&recv, start_ts, Some(handle))
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
        assert!(vm.run_cmd_and_wait(command, &recv, Instant::now()).is_err());

        // reset timeout for all tests below that shouldn't timeout
        vm.args.timeout_secs = Some(5);

        // command completes with success
        let (_send, recv) = UnixStream::pair().expect("Failed to create sockets");
        let command = Command::new("/usr/bin/ls");
        let result = vm.run_cmd_and_wait(command, &recv, Instant::now());
        println!("{:?}", result);
        assert!(result.is_ok());
        assert!(result.expect("Already checked").success());

        // command completes with non-zero exit code
        let (_send, recv) = UnixStream::pair().expect("Failed to create sockets");
        let mut command = Command::new("/usr/bin/ls");
        command.arg("non_existent_file");
        let result = vm.run_cmd_and_wait(command, &recv, Instant::now());
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
        let mount_platform = MountPlatformDecision(true);
        Platform::set(&mount_platform).expect("Failed to query platform");

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
