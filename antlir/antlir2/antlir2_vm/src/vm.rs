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
use std::thread;
use std::time::Duration;
use std::time::Instant;

use thiserror::Error;
use tracing::debug;
use tracing::info;

use crate::disk::QCow2Disk;
use crate::disk::QCow2DiskBuilder;
use crate::disk::QCow2DiskError;
use crate::runtime::get_runtime;
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
    /// Directory to keep all ephemeral states
    state_dir: PathBuf,
}

#[derive(Error, Debug)]
pub(crate) enum VMError {
    #[error("Failed to create directory for VM states")]
    StateDirError(std::io::Error),
    #[error(transparent)]
    DiskInitError(#[from] QCow2DiskError),
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
    pub(crate) fn new(opts: VMOpts) -> Result<Self> {
        let state_dir = Self::create_state_dir()?;
        let disks = Self::create_disks(&opts, &state_dir)?;

        Ok(VM {
            opts,
            disks,
            state_dir,
        })
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

    pub(crate) fn run(&mut self) -> Result<()> {
        self.spawn_vm()?;
        self.wait_for_vm()?;
        Ok(())
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
        args.extend(self.disk_qemu_args());
        log_command(Command::new(&get_runtime().qemu_system).args(&args))
            .spawn()
            .map_err(VMError::QemuProcessError)
    }

    /// Connect to the notify socket, wait for boot ready message and wait for the VM
    /// to terminate. If time out is specified, this function will return error
    /// upon timing out.
    fn wait_for_vm(&self) -> Result<()> {
        let start_ts = Instant::now();

        // Wait for notify file to be created by qemu
        debug!("Waiting for notify file to be created");
        while !self.time_left(start_ts)?.is_zero() {
            match self.notify_file().try_exists() {
                Ok(true) => break,
                Ok(false) => thread::sleep(Duration::from_millis(100)),
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

        // VM booted. Closing the notify socket will result in its termination. If VM
        // terminates on its own, the socket will be closed. So we poll the notify
        // socket until timeout even though there won't be data coming.
        let mut buf = Vec::new();
        let mut socket = f.into_inner();
        match self.opts.args.timeout_s {
            Some(_) => {
                socket.set_nonblocking(true).map_err(|e| {
                    VMError::BootError(format!("Failed to set non-blocking socket option: {e}"))
                })?;
                while !self.time_left(start_ts)?.is_zero() {
                    match socket.read(&mut buf) {
                        Ok(0) => {
                            debug!("Notify socket closed. VM exited");
                            break;
                        }
                        Ok(_) => debug!("Received unexpected data"),
                        Err(_) => thread::sleep(Duration::from_secs(1)),
                    }
                }
            }
            None => socket
                .read_to_end(&mut buf)
                .map(|_| ())
                .map_err(VMError::RunError)?,
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
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::runtime::set_runtime;
    use crate::types::RuntimeOpts;
    use crate::types::VMArgs;

    fn get_vm_no_disk() -> VM {
        let opts = VMOpts {
            cpus: 1,
            mem_mib: 1024,
            disks: vec![],
            args: VMArgs { timeout_s: None },
        };
        VM {
            opts,
            disks: vec![],
            state_dir: PathBuf::from("/test/path"),
        }
    }

    fn set_bogus_runtime() {
        set_runtime(RuntimeOpts {
            qemu_system: "qemu-system".to_string(),
            qemu_img: "qemu-img".to_string(),
            firmware: "edk2-x86_64-code.fd".to_string(),
            roms_dir: "roms".to_string(),
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
}
