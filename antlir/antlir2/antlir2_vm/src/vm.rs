/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use thiserror::Error;
use tracing::debug;
use tracing::info;

use crate::disk::QCow2Disk;
use crate::disk::QCow2DiskBuilder;
use crate::disk::QCow2DiskError;
use crate::isolation::Platform;
use crate::net::VirtualNIC;
use crate::net::VirtualNICError;
use crate::runtime::get_runtime;
use crate::share::ShareError;
use crate::share::ShareOpts;
use crate::share::Shares;
use crate::share::VirtiofsShare;
use crate::ssh::GuestSSHCommand;
use crate::ssh::GuestSSHError;
use crate::types::VMOpts;
use crate::utils::log_command;
use crate::utils::NodeNameCounter;

#[derive(Debug)]
pub(crate) struct VM {
    /// VM specification
    opts: VMOpts,
    /// List of writable drives created for the VM. We need to hold the ownership
    /// to prevent the temporary disks from getting cleaned up prematuresly.
    disks: Vec<QCow2Disk>,
    /// All directories to be shared into the VM
    shares: Shares,
    /// Virtual NICs to create and attach
    nics: Vec<VirtualNIC>,
    /// Directory to keep all ephemeral states
    state_dir: PathBuf,
}

#[derive(Error, Debug)]
pub(crate) enum VMError {
    #[error("Failed to create directory for VM states")]
    StateDirError(std::io::Error),
    #[error(transparent)]
    DiskInitError(#[from] QCow2DiskError),
    #[error(transparent)]
    ShareInitError(#[from] ShareError),
    #[error(transparent)]
    NICInitError(#[from] VirtualNICError),
    #[error(transparent)]
    SSHCommandError(#[from] GuestSSHError),
    #[error("Failed to spawn qemu process: `{0}`")]
    QemuProcessError(std::io::Error),
    #[error("Failed to boot VM: `{0}`")]
    BootError(String),
    #[error("VM error after boot: `{0}`")]
    RunError(std::io::Error),
    #[error("VM timed out")]
    TimeOutError,
}

type Result<T> = std::result::Result<T, VMError>;

impl VM {
    /// Create a new VM along with its virtual resources
    pub(crate) fn new(opts: VMOpts) -> Result<Self> {
        let state_dir = Self::create_state_dir()?;
        let disks = Self::create_disks(&opts, &state_dir)?;
        let shares = Self::create_shares(&state_dir, opts.mem_mib)?;
        let nics = Self::create_nics(opts.num_nics)?;

        Ok(VM {
            opts,
            disks,
            shares,
            nics,
            state_dir,
        })
    }

    /// Run the VM and wait for it to finish
    pub(crate) fn run(&mut self) -> Result<()> {
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

    /// Create all writable disks
    fn create_disks(opts: &VMOpts, state_dir: &Path) -> Result<Vec<QCow2Disk>> {
        let mut vd_counter = NodeNameCounter::new("vd");
        opts.disks
            .iter()
            .map(|x| {
                Ok(QCow2DiskBuilder::default()
                    .opts(x.clone())
                    .name(vd_counter.next())
                    .state_dir(state_dir.to_path_buf())
                    .build()?)
            })
            .collect()
    }

    /// Create all shares for the platform and generate all necessary unit files
    fn create_shares(state_dir: &Path, mem_mb: usize) -> Result<Shares> {
        let platform_shares: Result<Vec<_>> = Platform::get()
            .iter()
            .enumerate()
            .map(|(i, d)| -> Result<VirtiofsShare> {
                let opts = ShareOpts {
                    path: d.to_str().expect("Invalid share path").to_string(),
                    read_only: true,
                    mount_tag: None,
                };
                let share = VirtiofsShare::new(opts, i, state_dir.to_path_buf());
                share.start_virtiofsd()?;
                Ok(share)
            })
            .collect();
        let unit_files_dir = state_dir.join("mount_units");
        fs::create_dir(&unit_files_dir).map_err(VMError::StateDirError)?;
        let shares = Shares::new(platform_shares?, mem_mb, unit_files_dir)?;
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
        match self.opts.args.timeout_s {
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

    /// Spawn qemu-system process. It won't immediately start running until we connect
    /// to the notify socket.
    fn spawn_vm(&self) -> Result<Child> {
        let mut args = self.common_qemu_args()?;
        args.extend(self.non_disk_boot_qemu_args());
        args.extend(self.disk_qemu_args());
        args.extend(self.share_qemu_args());
        args.extend(self.nic_qemu_args());
        let mut command = Command::new(&get_runtime().qemu_system);
        let command = command.args(&args);
        if !self.opts.args.console {
            command.stdin(Stdio::null());
            command.stdout(Stdio::null());
            command.stderr(Stdio::null());
        }
        log_command(command)
            .spawn()
            .map_err(VMError::QemuProcessError)
    }

    /// Closing the notify socket will result in VM's termination. If VM
    /// terminates on its own, the socket will be closed. So we poll the notify
    /// socket until timeout.
    fn wait_for_timeout(&self, socket: &mut UnixStream, start_ts: Instant) -> Result<()> {
        if self.opts.args.timeout_s.is_some() {
            // Poll until either socket close or timeout. The buffer size is arbitrary,
            // because we don't expect any data.
            let mut buf = [0; 8];
            socket.set_nonblocking(true).map_err(|e| {
                VMError::BootError(format!("Failed to set non-blocking socket option: {e}"))
            })?;
            while !self.time_left(start_ts)?.is_zero() {
                match socket.read(&mut buf) {
                    Ok(0) => {
                        debug!("Notify socket closed. VM exited");
                        break;
                    }
                    Ok(_) => debug!("Received unexpected data from VM notify socket"),
                    Err(_) => thread::sleep(Duration::from_secs(1)),
                }
            }
        } else {
            // Block until socket close without timeout
            let mut buf = Vec::new();
            socket.read_to_end(&mut buf).map_err(VMError::RunError)?;
        }
        Ok(())
    }

    /// We control VM process through sockets. If VM process exited for any reason
    /// before socket connection is established, it's an error. Detect such early
    /// failure by polling process status.
    fn try_wait_vm_proc(&self, child: &mut Child) -> Result<()> {
        match child.try_wait() {
            Ok(Some(status)) => Err(VMError::BootError(format!(
                "VM process exited prematurely: {status}"
            ))),
            Ok(None) => Ok(()),
            Err(e) => Err(VMError::BootError(format!(
                "Error attempting to wait for VM process: {e}"
            ))),
        }
    }

    /// Connect to the notify socket, wait for boot ready message and wait for the VM
    /// to terminate. If time out is specified, this function will return error
    /// upon timing out.
    fn wait_for_vm(&self, mut vm_proc: Child) -> Result<()> {
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
                Err(e) => {
                    return Err(VMError::BootError(format!(
                        "Unable to access notify file {e}"
                    )));
                }
            }
        }

        // Connect to the notify socket. This starts the boot process.
        let socket = UnixStream::connect(self.notify_file())
            .map_err(|e| VMError::BootError(format!("Failed to connect to notify socket {e}")))?;

        // Wait for boot notify message. We expect "READY" message once VM boots
        debug!("Waiting for boot notify message");
        if self.opts.args.timeout_s.is_some() {
            socket
                .set_read_timeout(Some(self.time_left(start_ts)?))
                .map_err(|e| {
                    VMError::BootError(format!("Failed to set notify socket read timeout: {e}"))
                })?;
        }
        let mut response = String::new();
        let mut f = BufReader::new(socket);
        f.read_line(&mut response)
            .map_err(|e| VMError::BootError(format!("Failed to read notify socket: {e}")))?;
        debug!(
            "Received boot event {} after {} seconds",
            response.trim(),
            start_ts.elapsed().as_secs_f32()
        );

        // VM booted
        if !self.opts.args.console {
            // If we want a shell, open it now and don't timeout. We don't care about return
            // status of the ssh command in this case.
            let mut ssh_cmd = GuestSSHCommand::new()?.ssh_cmd(None, None);
            log_command(&mut ssh_cmd)
                .status()
                .map_err(|e| VMError::BootError(format!("Failed to open SSH shell: {}", e)))?;
        } else {
            // Otherwise, we are running some command or tests. We spawn the command and
            // wait for its completion or timeout.
            // TODO: tests are not implemented yet.
            let mut socket = f.into_inner();
            self.wait_for_timeout(&mut socket, start_ts)?;
        }

        info!("VM executed for {} seconds", start_ts.elapsed().as_secs());
        Ok(())
    }

    fn common_qemu_args(&self) -> Result<Vec<String>> {
        Ok([
            // General
            "-no-reboot",
            "-display",
            "none",
            // Serial
            "-serial",
            "mon:stdio",
            // CPU & Memory
            "-cpu",
            "host",
            "-smp",
            &self.opts.cpus.to_string(),
            "-m",
            &format!("{}M", self.opts.mem_mib),
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
            // Assuming we won't bother without virtualization support
            "-enable-kvm",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect())
    }

    fn disk_qemu_args(&self) -> Vec<String> {
        self.disks.iter().flat_map(|x| x.qemu_args()).collect()
    }

    fn non_disk_boot_qemu_args(&self) -> Vec<String> {
        match &self.opts.non_disk_boot_opts {
            Some(opts) => [
                "-initrd",
                &opts.initrd,
                // kernel
                "-kernel",
                &opts.kernel,
                "-append",
                &[
                    "console=ttyS0,115200",
                    "panic=-1",
                    "audit=0",
                    "selinux=0",
                    "systemd.hostname=vmtest",
                    "net.ifnames=1",
                    "root=LABEL=/",
                    // kernel args
                    "rootflags=subvol=volume",
                    "rootfstype=btrfs",
                    &opts.append,
                ]
                .join(" "),
            ]
            .iter()
            .map(|x| x.to_string())
            .collect(),
            None => vec![],
        }
    }

    fn share_qemu_args(&self) -> Vec<String> {
        self.shares.qemu_args()
    }

    fn nic_qemu_args(&self) -> Vec<String> {
        self.nics.iter().flat_map(|x| x.qemu_args()).collect()
    }
}

#[cfg(test)]
mod test {
    use std::net::Shutdown;
    use std::thread;

    use regex::Regex;

    use super::*;
    use crate::runtime::set_runtime;
    use crate::types::NonDiskBootOpts;
    use crate::types::RuntimeOpts;
    use crate::types::VMArgs;

    fn get_vm_no_disk() -> VM {
        let opts = VMOpts {
            cpus: 1,
            mem_mib: 1024,
            disks: vec![],
            num_nics: 1,
            non_disk_boot_opts: None,
            args: VMArgs {
                timeout_s: None,
                console: false,
            },
        };
        let share_opts = ShareOpts {
            path: "/path".to_string(),
            read_only: true,
            mount_tag: None,
        };
        let share = VirtiofsShare::new(share_opts, 1, PathBuf::from("/state"));
        VM {
            opts,
            disks: vec![],
            shares: Shares::new(vec![share], 1024, PathBuf::from("/state/units"))
                .expect("Failed to create Shares"),
            nics: vec![VirtualNIC::new(0)],
            state_dir: PathBuf::from("/test/path"),
        }
    }

    fn set_bogus_runtime() {
        set_runtime(RuntimeOpts {
            qemu_system: "qemu-system".to_string(),
            qemu_img: "qemu-img".to_string(),
            firmware: "edk2-x86_64-code.fd".to_string(),
            roms_dir: "roms".to_string(),
            virtiofsd: "virtiofsd".to_string(),
        })
        .expect("Failed to set fake runtime");
    }

    #[test]
    fn test_common_qemu_args() {
        let vm = get_vm_no_disk();
        set_bogus_runtime();
        let common_args = vm
            .common_qemu_args()
            .expect("Failed to get qemu args")
            .join(" ");
        // Only checking fields affected by args
        assert!(common_args.contains("-cpu host -smp 1"));
        assert!(common_args.contains("-m 1024M"));
        assert!(common_args.contains(&format!(
            "-chardev socket,path={}/vmtest_notify.sock,id=notify,server=on",
            vm.state_dir.to_str().expect("Invalid tempdir path"),
        )));
        assert!(
            common_args
                .contains("if=pflash,format=raw,unit=0,file=edk2-x86_64-code.fd,readonly=on")
        );
        assert!(common_args.contains("-L roms"));
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
        vm.opts.args.timeout_s = Some(60);
        let start_ts = Instant::now();
        assert!(vm.time_left(start_ts).expect("Unexpected timeout") > Duration::from_secs(1));
        vm.opts.args.timeout_s = Some(1);
        thread::sleep(Duration::from_secs(1));
        assert!(vm.time_left(start_ts).is_err());
    }

    #[test]
    fn test_non_boot_qemu_args() {
        let mut vm = get_vm_no_disk();
        assert_eq!(vm.non_disk_boot_qemu_args(), Vec::<String>::new());

        vm.opts.non_disk_boot_opts = Some(NonDiskBootOpts {
            initrd: "initrd".to_string(),
            kernel: "kernel".to_string(),
            append: "whatever".to_string(),
        });
        let args = vm.non_disk_boot_qemu_args().join(" ");
        assert!(args.contains("-initrd initrd"));
        assert!(args.contains("-kernel kernel"));
        let re = Regex::new("-append .* whatever").expect("Failed to get regex");
        assert!(re.is_match(&args));
    }

    #[test]
    fn test_wait_for_timeout() {
        // Terminate after timeout
        let mut vm = get_vm_no_disk();
        vm.opts.args.timeout_s = Some(3);
        let (_send, mut recv) = UnixStream::pair().expect("Failed to create sockets");
        let start_ts = Instant::now();
        let handle = thread::spawn(move || {
            assert!(vm.wait_for_timeout(&mut recv, start_ts).is_err());
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
        vm.opts.args.timeout_s = Some(10);
        let (send, mut recv) = UnixStream::pair().expect("Failed to create sockets");
        let start_ts = Instant::now();
        let handle = thread::spawn(move || {
            assert!(vm.wait_for_timeout(&mut recv, start_ts).is_ok());
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
        vm.opts.args.timeout_s = None;
        let (send, mut recv) = UnixStream::pair().expect("Failed to create sockets");
        let handle = thread::spawn(move || {
            assert!(vm.wait_for_timeout(&mut recv, Instant::now()).is_ok());
        });
        thread::sleep(Duration::from_secs(1));
        assert!(!handle.is_finished());
        send.shutdown(Shutdown::Both)
            .expect("Failed to shutdown sender");
        handle.join().expect("Test thread panic'ed");
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
}
