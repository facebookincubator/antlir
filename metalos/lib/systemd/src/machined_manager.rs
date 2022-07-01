/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use slog::Logger;
use zbus::dbus_proxy;
use zvariant::derive::Type;
use zvariant::Signature;
use zvariant::Type;

use crate::dbus_types::*;
use crate::systemd_manager::UnitName;
use crate::ConnectOpts;
use crate::Result;
use crate::Systemd;
use systemd_macros::SystemdEnum;
use systemd_macros::TransparentZvariant;

#[async_trait]
pub trait MachineExt {
    /// Connect to the systemd manager api within the running container using
    /// the default connection settings.
    async fn systemd(&self, log: Logger) -> Result<Systemd> {
        self.systemd_with_opts(log, ConnectOpts::default()).await
    }

    /// Connect to the systemd manager api within the running container.
    async fn systemd_with_opts(&self, log: Logger, mut opts: ConnectOpts) -> Result<Systemd>;
}

#[async_trait]
impl MachineExt for MachineProxy<'static> {
    async fn systemd_with_opts(&self, log: Logger, mut opts: ConnectOpts) -> Result<Systemd> {
        // NOTE: it would be really nice if we could properly `setns` to get
        // into the mount namespace of the Leader process, but `setns` cannot be
        // used if the calling program has threads, which we do. However, we can
        // use /proc/$LEADER/root to get at the dbus socket without having to
        // actually enter the namespace
        opts.dbus_addr = format!(
            "unix:path=/proc/{}/root/run/dbus/system_bus_socket",
            self.leader().await?
        );
        Systemd::connect_with_opts(log, opts).await
    }
}

#[derive(Debug, PartialEq, Eq, Clone, TransparentZvariant)]
pub struct MachineName(String);

#[derive(Debug, PartialEq, Eq, Clone, TransparentZvariant)]
pub struct ImageName(String);

/// PID of a machine's leader process.
#[derive(Debug, PartialEq, Eq, Clone, TransparentZvariant)]
pub struct Leader(u32);

#[derive(Debug, PartialEq, Eq, Clone, TransparentZvariant)]
pub struct Size(u64);

impl Size {
    pub fn as_bytes(&self) -> u64 {
        self.0
    }

    pub fn from_bytes(size: u64) -> Self {
        Self(size)
    }
}

/// Virtualization technology used by a machine.
#[derive(Debug, PartialEq, Eq, Copy, Clone, SystemdEnum)]
pub enum Class {
    /// Real VMs based on virtualized hardware
    Vm,
    /// Light-weight userspace virtualization sharing the same kernel as the host
    Container,
}

/// The running state of a registered machine.
#[derive(Debug, PartialEq, Eq, Clone, SystemdEnum)]
pub enum MachineState {
    Opening,
    Running,
    Closing,
    /// Note that the state machine is not considered part of the API
    /// and states might be removed or added without this being considered API
    /// breakage.
    Unknown(String),
}

/// Some info about a machine as returned by
/// [list_machines](ManagerProxy::list_machines).
#[derive(Debug, PartialEq, Eq, Deserialize, Type)]
pub struct ListedMachine {
    pub name: MachineName,
    pub class: Class,
    pub service: String,
    pub path: TypedObjectPath<MachineProxy<'static>>,
}

/// Some info about an image as returned by
/// [list_images](ManagerProxy::list_images).
#[derive(Debug, PartialEq, Eq, Deserialize, Type)]
pub struct ListedImage {
    pub name: ImageName,
    pub image_type: String,
    pub readonly: bool,
    pub created_time: Timestamp,
    pub modified_time: Timestamp,
    pub path: TypedObjectPath<ImageProxy<'static>>,
}

/// When sending a signal to a machine, control which processe(s) get sent the
/// signal.
#[derive(Debug, PartialEq, Eq, Clone, SystemdEnum)]
pub enum KillWho {
    Leader,
    All,
}

/// IP Address assigned to a machine.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Address(std::net::IpAddr);

impl Type for Address {
    fn signature() -> Signature<'static> {
        <Vec<u8> as Type>::signature()
    }
}

#[dbus_proxy(
    interface = "org.freedesktop.machine1.Manager",
    default_service = "org.freedesktop.machine1",
    default_path = "/org/freedesktop/machine1",
    gen_blocking = false
)]
trait Manager {
    /// Bind mount a file or directory from the host into the container. Its
    /// arguments consist of a machine name, the source directory on the host,
    /// the destination directory in the container, and two booleans, one
    /// indicating whether the bind mount shall be read-only, the other
    /// indicating whether the destination mount point shall be created first,
    /// if it is missing.
    fn bind_mount_machine(
        &self,
        name: &MachineName,
        source: &FilePath,
        destination: &FilePath,
        read_only: bool,
        mkdir: bool,
    ) -> zbus::Result<()>;

    /// Clones the specified image under a new name. It also takes a boolean
    /// argument indicating whether the resulting image shall be read-only or
    /// not.
    fn clone_image(
        &self,
        name: &ImageName,
        new_name: &ImageName,
        read_only: bool,
    ) -> zbus::Result<()>;

    /// Copy files or directories from a container into the host. It takes a
    /// container name, a source directory in the container and a destination
    /// directory on the host as arguments.
    fn copy_from_machine(
        &self,
        name: &MachineName,
        source: &FilePath,
        destination: &FilePath,
    ) -> zbus::Result<()>;

    /// Inverse of [copy_from_machine](ManagerProxy::copy_from_machine).
    fn copy_to_machine(
        &self,
        name: &MachineName,
        source: &FilePath,
        destination: &FilePath,
    ) -> zbus::Result<()>;

    /// Get the image object path of the image with the specified name.
    fn get_image(&self, name: &ImageName) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// Get the machine object for the machine with the specified name.
    #[dbus_proxy(object = "Machine")]
    fn get_machine(&self, name: &MachineName);

    /// Retrieve the IP addresses of a container. This method returns an array
    /// of pairs consisting of an address family specifier (AF_INET or AF_INET6)
    /// and a byte array containing the addresses. This is only supported for
    /// containers that make use of network namespacing.
    fn get_machine_addresses(&self, name: &MachineName) -> zbus::Result<Vec<(i32, Address)>>;

    /// Similarly, [get_machine_by_pid](ManagerProxy::get_machine_by_pid) gets
    /// the machine object the specified PID belongs to if there is any.
    #[dbus_proxy(object = "Machine")]
    fn get_machine_by_pid(&self, pid: u32);

    /// Retrieve the OS release information of a container. This method returns
    /// an array of key value pairs read from the os-release(5) file in the
    /// container and is useful to identify the operating system used in a
    /// container.
    fn get_machine_osrelease(
        &self,
        name: &MachineName,
    ) -> zbus::Result<std::collections::HashMap<String, String>>;

    /// Send a UNIX signal to (some of) the machine's processes.
    fn kill_machine(&self, name: &MachineName, who: &KillWho, signal: Signal) -> zbus::Result<()>;

    /// Get an array of all currently known images.
    fn list_images(&self) -> zbus::Result<Vec<ListedImage>>;

    /// Get an array of all currently registered machines.
    fn list_machines(&self) -> zbus::Result<Vec<ListedMachine>>;

    /// Change the read-only flag of an image.
    fn mark_image_read_only(&self, name: &ImageName, read_only: bool) -> zbus::Result<()>;

    /// Allocates a pseudo TTY in the container and ensures that a getty login
    /// prompt of the container is running on the other end.  It returns the
    /// file descriptor of the PTY and the PTY path. This is useful for
    /// acquiring a pty with a login prompt from the container.
    fn open_machine_login(
        &self,
        name: &MachineName,
    ) -> zbus::Result<(zbus::zvariant::Fd, OwnedFilePath)>;

    /// Allocates a pseudo TTY in the container and returns a file descriptor
    /// and its path. This is equivalent to transitioning into the container and
    /// invoking posix_openpt(3).
    fn open_machine_pty(
        &self,
        name: &MachineName,
    ) -> zbus::Result<(zbus::zvariant::Fd, OwnedFilePath)>;

    /// Return a file descriptor of the machine's root directory. See
    /// [root_directory](MachineProxy::root_directory).
    fn open_machine_root_directory(&self, name: &MachineName) -> zbus::Result<zbus::zvariant::Fd>;

    /// Allocates a pseudo TTY in the container, as the specified user, and
    /// invokes the executable at the specified path with a list of arguments
    /// (starting from argv\[0\]) and an environment block. It then returns the
    /// file descriptor of the PTY and the PTY path.
    fn open_machine_shell(
        &self,
        name: &MachineName,
        user: &str,
        path: &FilePath,
        args: &[&str],
        environment: &Environment,
    ) -> zbus::Result<(zbus::zvariant::Fd, OwnedFilePath)>;

    /// Delete a registered image.
    fn remove_image(&self, name: &ImageName) -> zbus::Result<()>;

    /// Change the name of an image.
    fn rename_image(&self, name: &ImageName, new_name: &ImageName) -> zbus::Result<()>;

    /// Set a per-image quota limit.
    fn set_image_limit(&self, name: &ImageName, size: Size) -> zbus::Result<()>;

    /// Set an overall quota limit on the pool of images.
    fn set_pool_limit(&self, size: Size) -> zbus::Result<()>;

    /// Terminate a virtual machine, killing its processes.  It takes a machine
    /// name as its only argument.
    fn terminate_machine(&self, name: &MachineName) -> zbus::Result<()>;

    /// Sent whenever a machine is registered.
    #[dbus_proxy(signal)]
    fn machine_new(
        &self,
        id: MachineName,
        machine: TypedObjectPath<MachineProxy<'_>>,
    ) -> zbus::Result<()>;

    /// Sent whenever a machine is removed.
    #[dbus_proxy(signal)]
    fn machine_removed(
        &self,
        id: MachineName,
        machine: TypedObjectPath<MachineProxy<'_>>,
    ) -> zbus::Result<()>;

    /// Size limit of the image pool in bytes.
    #[dbus_proxy(property)]
    fn pool_limit(&self) -> zbus::Result<Size>;

    /// File system path where images are written to.
    #[dbus_proxy(property)]
    fn pool_path(&self) -> zbus::Result<OwnedFilePath>;

    /// Current usage size of the image pool in bytes.
    #[dbus_proxy(property)]
    fn pool_usage(&self) -> zbus::Result<Size>;
}

#[dbus_proxy(
    interface = "org.freedesktop.machine1.Machine",
    default_service = "org.freedesktop.machine1",
    gen_blocking = false
)]
trait Image {
    #[dbus_proxy(property)]
    fn name(&self) -> zbus::Result<ImageName>;

    #[dbus_proxy(property)]
    fn path(&self) -> zbus::Result<OwnedFilePath>;

    #[dbus_proxy(property, name = "Type")]
    fn image_type(&self) -> zbus::Result<String>;
}

/// DBus interface for individual Machine objects
#[dbus_proxy(
    interface = "org.freedesktop.machine1.Machine",
    default_service = "org.freedesktop.machine1",
    gen_blocking = false
)]
trait Machine {
    /// See [bind_mount_machine](ManagerProxy::bind_mount_machine)
    fn bind_mount(
        &self,
        source: &FilePath,
        destination: &FilePath,
        read_only: bool,
        mkdir: bool,
    ) -> zbus::Result<()>;

    /// See [copy_from_machine](ManagerProxy::copy_from_machine)
    fn copy_from(&self, source: &FilePath, destination: &FilePath) -> zbus::Result<()>;

    /// See [copy_to_machine](ManagerProxy::copy_to_machine)
    fn copy_to(&self, source: &FilePath, destination: &FilePath) -> zbus::Result<()>;

    /// See [get_machine_addresses](ManagerProxy::get_machine_addresses)
    fn get_addresses(&self) -> zbus::Result<Vec<(i32, Address)>>;

    /// See [get_machine_osrelease](ManagerProxy::get_machine_osrelease)
    fn get_osrelease(&self) -> zbus::Result<std::collections::HashMap<String, String>>;

    /// See [kill_machine](ManagerProxy::kill_machine)
    fn kill(&self, who: &KillWho, signal: Signal) -> zbus::Result<()>;

    /// See [open_machine_login](ManagerProxy::open_machine_login)
    fn open_login(&self) -> zbus::Result<(zbus::zvariant::Fd, OwnedFilePath)>;

    /// See [open_machine_pty](ManagerProxy::open_machine_pty)
    fn open_pty(&self) -> zbus::Result<(zbus::zvariant::Fd, OwnedFilePath)>;

    /// See [open_machine_root_directory](ManagerProxy::open_machine_root_directory)
    fn open_root_directory(&self) -> zbus::Result<zbus::zvariant::Fd>;

    /// See [open_machine_shell](ManagerProxy::open_machine_shell)
    fn open_shell(
        &self,
        user: &str,
        path: &FilePath,
        args: &[&str],
        environment: &Environment,
    ) -> zbus::Result<(zbus::zvariant::Fd, OwnedFilePath)>;

    /// See [terminate_machine](ManagerProxy::terminate_machine)
    fn terminate(&self) -> zbus::Result<()>;

    #[dbus_proxy(property)]
    fn class(&self) -> zbus::Result<Class>;

    /// Machine UUID.
    #[dbus_proxy(property)]
    fn id(&self) -> zbus::Result<Uuid>;

    /// PID of the leader process of the machine.
    #[dbus_proxy(property)]
    fn leader(&self) -> zbus::Result<Leader>;

    /// Machine name as it was passed in during registration.
    #[dbus_proxy(property)]
    fn name(&self) -> zbus::Result<MachineName>;

    /// Array of network interface indices that point towards the container, the
    /// VM or the host.
    #[dbus_proxy(property)]
    fn network_interfaces(&self) -> zbus::Result<Vec<i32>>;

    /// Root directory of the container if it is known and applicable or the
    /// empty string.
    #[dbus_proxy(property)]
    fn root_directory(&self) -> zbus::Result<OwnedFilePath>;

    /// Short string identifying the registering service as passed in during
    /// registration of the machine.
    #[dbus_proxy(property)]
    fn service(&self) -> zbus::Result<String>;

    /// Current running state of the machine.
    #[dbus_proxy(property)]
    fn state(&self) -> zbus::Result<MachineState>;

    /// Realtime timestamp when the virtual machine was created.
    #[dbus_proxy(property)]
    fn timestamp(&self) -> zbus::Result<Timestamp>;

    /// Monotonic version of [timestamp](MachineProxy::timestamp)
    #[dbus_proxy(property)]
    fn timestamp_monotonic(&self) -> zbus::Result<MonotonicTimestamp>;

    /// Unit is the systemd scope or service unit name for the machine.
    #[dbus_proxy(property)]
    fn unit(&self) -> zbus::Result<UnitName>;
}

#[cfg(test)]
mod tests {
    use super::MachineExt;
    use super::Size;
    use crate::Machined;
    use crate::Systemd;
    use crate::WaitableSystemState;
    use anyhow::Result;
    use byteorder::LE;
    use std::time::Duration;
    use tokio::time::sleep;
    use tokio::time::timeout;
    use zvariant::from_slice;
    use zvariant::to_bytes;
    use zvariant::EncodingContext as Context;

    #[containertest]
    async fn test_machine_api() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log.clone()).await?;
        sd.wait(WaitableSystemState::Operational).await?;
        let mut container = std::process::Command::new("systemd-nspawn")
            .arg("--directory=/")
            .arg("--boot")
            .arg("--ephemeral")
            .arg("--machine=containermachine")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let md = Machined::connect(log.clone()).await?;

        let machine = timeout(Duration::from_secs(1), async {
            loop {
                match md.get_machine(&"containermachine".into()).await {
                    Ok(machine) => return machine,
                    Err(e) => {
                        eprintln!("machine still not up: {:?}", e);
                        sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        })
        .await?;
        let machine_sd = machine.systemd(log).await?;
        machine_sd.wait(WaitableSystemState::Operational).await?;
        let _ = container.kill();
        Ok(())
    }

    #[test]
    fn test_size() -> Result<()> {
        let ctxt = Context::<LE>::new_dbus(0);

        let encoded = to_bytes(ctxt, &100000u64)?;
        let decoded: Size = from_slice(&encoded, ctxt)?;
        assert_eq!(decoded.as_bytes(), 100000);

        let encoded = to_bytes(ctxt, &Size(100000))?;
        let decoded: u64 = from_slice(&encoded, ctxt)?;
        assert_eq!(decoded, 100000);
        Ok(())
    }
}
