/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::Path;
use std::str::FromStr;

use bitflags::bitflags;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use systemd_macros::SystemdEnum;
use systemd_macros::TransparentZvariant;
use zbus::dbus_proxy;
use zvariant::derive::Type;
use zvariant::OwnedValue;
use zvariant::Signature;
use zvariant::Type;

use crate::dbus_types::*;
use crate::system_state::SystemState;

#[derive(
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Clone,
    TransparentZvariant
)]
pub struct UnitName(String);

impl UnitName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<Path> for UnitName {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl AsRef<OsStr> for UnitName {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, TransparentZvariant)]
pub struct JobId(u32);

/// Install state of a unit file.
#[derive(Debug, PartialEq, Eq, SystemdEnum)]
pub enum UnitFileState {
    /// Unit file is permanently enabled.
    Enabled,
    /// Unit file is only temporarily enabled and will no longer be enabled
    /// after a reboot (that means, it is enabled via /run/ symlinks, rather
    /// than /etc/).
    EnabledRuntime,
    /// Unit file is linked into /etc/ permanently.
    Linked,
    /// Unit file is linked into /run/ temporarily (until the next reboot).
    LinkedRuntime,
    /// Unit file is masked permanently.
    Masked,
    /// Unit file is masked in /run/ temporarily (until the next reboot).
    MaskedRuntime,
    /// Unit is statically enabled. i.e. always enabled and doesn't need to be
    /// enabled explicitly.
    Static,
    /// Unit file is not enabled.
    Disabled,
    /// It could not be determined whether the unit file is enabled.
    Invalid,
    /// Unit file is symlinked so it can be referred to by another name.
    Alias,
    /// For forwards-compatibility if systemd adds any new values (for example,
    /// Alias is already missing from the documentation)
    Unknown(String),
}

/// Virtualization technology being used by the running system.
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Virtualization {
    /// No virtualization
    None,
    /// Full machine virtualization
    Vm(&'static str),
    /// Container virtualization
    Container(&'static str),
    /// There is some virtualization tech in play, but we don't know whether
    /// it's a vm or container. See systemd-detect-virt(1)
    Unknown(String),
}

impl Type for Virtualization {
    fn signature() -> Signature<'static> {
        String::signature()
    }
}

impl TryFrom<zvariant::OwnedValue> for Virtualization {
    type Error = zvariant::Error;

    fn try_from(v: zvariant::OwnedValue) -> zvariant::Result<Self> {
        // non-exhaustive list of virtualization types that are likely to be
        // cared about in MetalOS contexts
        String::try_from(v).map(|v| match v.as_str() {
            "qemu" => Self::Vm("qemu"),
            "kvm" => Self::Vm("kvm"),
            "amazon" => Self::Vm("amazon"),
            "vmware" => Self::Vm("vmware"),
            "lxc" => Self::Container("lxc"),
            "systemd-nspawn" => Self::Container("systemd-nspawn"),
            "docker" => Self::Container("docker"),
            "" => Self::None,
            _ => Self::Unknown(v),
        })
    }
}

#[derive(Debug, PartialEq, Eq, SystemdEnum)]
pub enum UnitFileChangeType {
    Symlink,
    Unlink,
}

/// The type of the change, the file name of the symlink and the destination of
/// the symlink.
pub type UnitFileChange = (UnitFileChangeType, String, OwnedFilePath);

#[derive(Debug, PartialEq, Eq, SystemdEnum)]
pub enum KillWhom {
    /// Only the main process of the unit will be killed.
    Main,
    /// Only the control process of the unit will be killed.
    /// A "control" process is for example a process that is configured via
    /// ExecStop= and is spawned in parallel to the main daemon process in order
    /// to shut it down.
    Control,
    /// All processes of the unit will be killed
    All,
}

bitflags! {
    pub struct EnableDisableUnitFlags: u64 {
        /// Enable or disable the unit for runtime only (/run/),
        const RUNTIME = 1;
        /// Symlinks pointing to other units will be replaced if necessary.
        const FORCE = 1 << 1;
        /// Add or remove the symlinks in /etc/systemd/system.attached and
        /// /run/systemd/system.attached.
        const PORTABLE = 1 << 2;
    }
}

impl Type for EnableDisableUnitFlags {
    fn signature() -> Signature<'static> {
        u64::signature()
    }
}

impl Serialize for EnableDisableUnitFlags {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.bits)
    }
}

/// When systemd detects it is running on a system with certain problems, it
/// will set an appropriate taint flag. Taints may be used to lower the chance
/// of bogus bug reports.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, SystemdEnum)]
pub enum Taint {
    /// Set if /usr/ is not pre-mounted when systemd is first invoked.
    /// See [Booting Without /usr is
    /// Broken](https://freedesktop.org/wiki/Software/systemd/separate-usr-is-broken/)
    /// for details why this is bad.
    SplitUser,
    /// /etc/mtab is not a symlink to /proc/self/mounts as required.
    MtabNotSymlink,
    /// Control groups have not been enabled in the kernel.
    CgroupsMissing,
    /// The local RTC is configured to be in local time rather UTC.
    LocalHwclock,
    /// Forward compatibility for new taint states.
    Unknown(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct TaintSet(BTreeSet<Taint>);

impl std::ops::Deref for TaintSet {
    type Target = BTreeSet<Taint>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Type for TaintSet {
    fn signature() -> Signature<'static> {
        String::signature()
    }
}

impl TryFrom<OwnedValue> for TaintSet {
    type Error = zvariant::Error;

    fn try_from(v: OwnedValue) -> zvariant::Result<Self> {
        v.try_into().and_then(|s: String| {
            s.split(':')
                .map(|t| Taint::from_str(t))
                .collect::<Result<BTreeSet<_>, _>>()
                .map(TaintSet)
        })
    }
}

impl<'de> Deserialize<'de> for TaintSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        Ok(TaintSet(
            s.split(':')
                // Taint::from_str's error will have a useful message
                .map(|t| Taint::from_str(t).unwrap())
                .collect::<BTreeSet<_>>(),
        ))
    }
}

/// Reflects whether a unit is currently active or not.
/// See also [UnitProxy::sub_state] for more fine-grained unit-type specific
/// active states.
#[derive(Debug, PartialEq, Eq, SystemdEnum)]
pub enum ActiveState {
    /// Unit is active (obviously).
    Active,
    /// Unit is active and currently reloading its configuration.
    Reloading,
    /// Unit is inactive and the previous run was successful or no previous run
    /// has taken place yet.
    Inactive,
    /// Unit is inactive and the previous run was not successful. More
    /// information about the reason for this is available on the unit type
    /// specific interfaces, for example #[ServiceProxy::result].
    Failed,
    /// Unit has previously been inactive but is currently in the process of
    /// entering an active state.
    Activating,
    /// Unit is currently in the process of deactivation.
    Deactivating,
}

#[derive(Debug, PartialEq, Eq, SystemdEnum)]
pub enum StartMode {
    /// Start the unit and its dependencies, possibly replacing already queued
    /// jobs that conflict with it.
    Replace,
    /// Start the unit and its dependencies, but will fail if this would change
    /// an already queued job.
    Fail,
    /// Start the unit in question and terminate all units that aren't
    /// dependencies of it.
    Isolate,
    /// Start a unit but ignore all its dependencies. Not recommended.
    IgnoreDependencies,
    /// Start a unit but only ignore the requirement dependencies. Not
    /// recommended.
    IgnoreRequirements,
}

#[derive(Debug, PartialEq, Eq, Clone, SystemdEnum)]
pub enum JobResult {
    /// Successful execution of a job.
    Done,
    /// The job has been canceled (via [cancel_job)](ManagerProxy::cancel_job)
    /// before it finished execution.
    Canceled,
    /// The job timeout was reached.
    Timeout,
    /// The job failed.
    Failed,
    /// A job this job depended on failed and the job hence was removed as well.
    Dependency,
    /// A job was skipped because it didn't apply to the unit's current state.
    Skipped,
    /// Forwards compatibility with any new state enums
    Unknown(String),
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, SystemdEnum)]
pub enum JobState {
    /// Job is currently queued but has not begun to execute yet.
    Waiting,
    /// Job is currently being executed.
    Running,
}

#[derive(Debug, PartialEq, Eq, Clone, SystemdEnum)]
pub enum JobType {
    Start,
    VerifyActive,
    Stop,
    Reload,
    Restart,
    TryRestart,
    ReloadOrStart,
    Unknown(String),
}

/// Some information about a unit as reported by [list_units](ManagerProxy::list_units).
#[derive(Debug, PartialEq, Eq, Deserialize, Type)]
pub struct ListedUnit {
    /// The primary unit name
    pub name: UnitName,
    /// The human readable description string
    pub description: String,
    /// The load state (i.e. whether the unit file has been loaded successfully)
    pub load_state: LoadState,
    /// The active state (i.e. whether the unit is currently started or not)
    pub active_state: ActiveState,
    /// The sub state (a more fine-grained version of the active state that is
    /// specific to the unit type, which the active state is not)
    pub sub_state: String,
    // A unit that is being followed in its state by this unit, if there is
    // any, otherwise the empty string. NOTE(vmagro) I have no idea what
    // this means, and the docs are very unclear so I'm just choosing not to
    // expose it
    _following_unit: String,
    pub unit: TypedObjectPath<UnitProxy<'static>>,
    /// Queued job for this unit, if any.
    pub job_id: JobId,
    /// Job type, if any.
    pub job_type: JobType,
    /// Job object path, if any.
    pub job: TypedObjectPath<JobProxy<'static>>,
}

/// Reflects whether the configuration for a unit has been loaded.
/// Note that LoadState is fully orthogonal to [ActiveState] as units without
/// valid loaded configuration might be active (because configuration might have
/// been reloaded at a time where a unit was already active).
#[derive(Debug, PartialEq, Eq, Clone, SystemdEnum)]
pub enum LoadState {
    /// Configuration was successfully loaded.
    Loaded,
    /// Configuration failed to load. [UnitProxy::load_error] will contain
    /// information about the cause of the failure.
    Error,
    /// Unit is currently masked (i.e. symlinked to /dev/null or empty)
    Masked,
    /// Unit file does not exist (and possibly never did)
    NotFound,
    Unknown(String),
}

#[derive(Debug, PartialEq, Eq, Deserialize, Type)]
pub struct ListedJob {
    pub id: JobId,
    pub unit_name: UnitName,
    pub job_type: JobType,
    pub state: JobState,
    pub job: TypedObjectPath<JobProxy<'static>>,
    pub unit: TypedObjectPath<UnitProxy<'static>>,
}

#[derive(Debug, PartialEq, Eq, SystemdEnum)]
pub enum ServiceResult {
    /// It is useful to determine the reason a service failed if it is in the
    /// "failed" state (see ActiveState above). The following values are
    /// Unit did not fail.
    Success,
    /// Not enough resources were available to fork off and execute the service
    /// processes.
    Resources,
    /// Timeout occurred while executing a service operation.
    Timeout,
    /// Service process exited with an unclean exit code.
    ExitCode,
    /// Service process exited with an uncaught signal.
    Signal,
    /// Service process exited uncleanly and dumped core.
    CoreDump,
    /// Service did not send out watchdog ping messages often enough.
    Watchdog,
    /// Service has been started too frequently in a specific time frame (as
    /// configured in StartLimitInterval, StartLimitBurst).
    StartLimit,
}

#[derive(Debug, PartialEq, Eq, SystemdEnum)]
pub enum Marker {
    /// Restart this unit when enqueing marked jobs
    NeedsRestart,
    /// Reload this unit when enqueing marked jobs
    NeedsReload,
}

#[dbus_proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1",
    gen_blocking = false
)]
trait Manager {
    /// Bind mount new files or directories into a running service's mount
    /// namespace.
    fn bind_mount_unit(
        &self,
        name: &UnitName,
        source: &FilePath,
        destination: &FilePath,
        read_only: bool,
        mkdir: bool,
    ) -> zbus::Result<()>;

    /// Cancel a specific job identified by its numeric ID.
    /// This operation is also available as [JobProxy::cancel] exists primarily
    /// to reduce the necessary round trips to execute this operation. Note that
    /// this will not have any effect on jobs whose execution has already begun.
    fn cancel_job(&self, id: JobId) -> zbus::Result<()>;

    /// Flushes the job queue, removing all jobs that are still queued.
    /// Note that this does not have any effect on jobs whose execution has
    /// already begun. It only flushes jobs that are queued and have not yet
    /// begun execution.
    fn clear_jobs(&self) -> zbus::Result<()>;

    /// Inverse of [enable_unit_files](ManagerProxy::enable_unit_files)
    #[dbus_proxy(name = "DisableUnitFilesWithFlags")]
    fn disable_unit_files(
        &self,
        units: &[&UnitName],
        flags: EnableDisableUnitFlags,
    ) -> zbus::Result<Vec<UnitFileChange>>;

    /// Enable one or more units in the system (by creating symlinks to them in
    /// /etc/ or /run/).
    /// * `files`   - file names or full absolute paths (if the unit files are
    ///      residing outside the usual unit search path)
    ///
    /// Returns a boolean that signals whether any of the unit files contained
    /// any enablement information (i.e. an \[Install\]) section, and a Vec of
    /// [UnitFileChange]s made.
    #[dbus_proxy(name = "EnableUnitFilesWithFlags")]
    fn enable_unit_files(
        &self,
        files: &[&UnitName],
        flags: EnableDisableUnitFlags,
    ) -> zbus::Result<(bool, Vec<UnitFileChange>)>;

    /// Create reload/restart jobs for units which have been appropriately
    /// marked with `Markers` in
    /// [set_unit_properties)](ManagerProxy::set_unit_properties).
    /// This is equivalent to calling
    /// [try_restart_unit](ManagerProxy::try_restart_unit) or
    /// [reload_or_try_restart_unit](ManagerProxy::reload_or_try_restart_unit)
    /// for the marked units.
    fn enqueue_marked_jobs(&self) -> zbus::Result<Vec<TypedObjectPath<JobProxy<'_>>>>;

    /// Ask the manager to exit. This is not available for the system manager
    /// and is useful only for user session managers.
    fn exit(&self) -> zbus::Result<()>;

    /// Retrieve the name of the unit to which default.target is aliased.
    fn get_default_target(&self) -> zbus::Result<String>;

    /// Get the [JobProxy] for a specific job, identified by its id.
    #[dbus_proxy(object = "Job")]
    fn get_job(&self, id: JobId);

    /// Get the unit object proxy for a unit name.
    /// If a unit has not been loaded yet by this name this method will fail.
    #[dbus_proxy(object = "Unit")]
    fn get_unit(&self, name: &UnitName);

    /// Get a specialized [ServiceProxy] for the named unit.
    /// See [get_unit](ManagerProxy::get_unit).
    #[dbus_proxy(object = "Service", name = "GetUnit")]
    fn get_service_unit(&self, name: &UnitName);

    /// Get the [UnitProxy] that a process ID belongs to.
    /// The PID must refer to an existing system process.
    #[dbus_proxy(object = "Unit")]
    fn get_unit_by_pid(&self, pid: u32);

    /// Get the current enablement status of a specific unit file.
    fn get_unit_file_state(&self, file: &OwnedFilePath) -> zbus::Result<UnitFileState>;

    /// See [reboot](ManagerProxy::reboot)
    fn halt(&self) -> zbus::Result<()>;

    /// See [reboot](ManagerProxy::reboot)
    #[dbus_proxy(name = "KExec")]
    fn kexec(&self) -> zbus::Result<()>;

    /// Send a signal to all processes of a unit.
    fn kill_unit(&self, name: &UnitName, whom: &KillWhom, signal: Signal) -> zbus::Result<()>;

    /// Link unit files that are located outside of the usual unit search paths
    /// into the unit search path.
    fn link_unit_files(
        &self,
        files: &[&FilePath],
        runtime: bool,
        force: bool,
    ) -> zbus::Result<Vec<UnitFileChange>>;

    /// Get all currently queued jobs.
    /// Note that a job is only considered queued while it is starting, not
    /// while it's running.
    fn list_jobs(&self) -> zbus::Result<Vec<ListedJob>>;

    /// Returns an array of unit names and their enablement status. Note that
    /// [list_units](ManagerProxy::list_units) returns a list of units currently
    /// loaded into memory, while
    /// [list_unit_files](ManagerProxy::list_unit_files) returns a list of unit
    /// files that were found on disk. Note that while most units are read
    /// directly from a unit file with the same name, some units are not backed
    /// by files and some files (templates) cannot directly be loaded as units
    /// but need to be instantiated instead.
    fn list_unit_files(&self) -> zbus::Result<Vec<(OwnedFilePath, UnitFileState)>>;

    /// Get an array of all currently loaded units.
    /// Note that units may be known by multiple names at the same name, and
    /// hence there might be more unit names loaded than actual units behind
    /// them.
    fn list_units(&self) -> zbus::Result<Vec<ListedUnit>>;

    /// Similar to [get_unit](ManagerProxy::get_unit) but will load the unit
    /// from disk if possible and necessary.
    #[dbus_proxy(object = "Unit")]
    fn load_unit(&self, name: &UnitName);

    /// Similar to [get_unit](ManagerProxy::get_service_unit) but will load the unit
    /// from disk if possible and necessary.
    #[dbus_proxy(object = "Service", name = "LoadUnit")]
    fn load_service_unit(&self, name: &UnitName);

    /// Mask unit files.
    /// See [enable_unit_files](ManagerProxy::enable_unit_files) for details on
    /// the boolean flags.
    fn mask_unit_files(
        &self,
        files: &[&FilePath],
        runtime: bool,
        force: bool,
    ) -> zbus::Result<Vec<UnitFileChange>>;

    /// Mount new images into a running service mount namespace.
    fn mount_image_unit(
        &self,
        name: &UnitName,
        source: &FilePath,
        destination: &FilePath,
        read_only: bool,
        mkdir: bool,
        options: &[(&str, &str)],
    ) -> zbus::Result<()>;

    /// See [reboot](ManagerProxy::reboot)
    fn power_off(&self) -> zbus::Result<()>;

    /// [reboot](ManagerProxy::reboot), [power_off](ManagerProxy::power_off),
    /// [halt](ManagerProxy::halt), or [kexec](ManagerProxy::kexec) may be used
    /// to ask for immediate reboot, powering down, halt or kexec based reboot
    /// of the system. Note that this does not shut down any services and
    /// immediately transitions into the reboot process. These functions are
    /// normally only called as the last step of shutdown and should not be
    /// called directly.  To shut down the machine, it is generally a better
    /// idea to invoke [reboot](ManagerProxy::reboot) or
    /// [power_off](ManagerProxy::power_off) on the systemd-logind manager
    /// object; see org.freedesktop.login1(5) for more information.
    fn reboot(&self) -> zbus::Result<()>;

    /// Reexecute the main manager process. It will serialize its state,
    /// reexecute, and deserizalize the state again.  This is useful for
    /// upgrades and is a more comprehensive version of
    /// [reload](ManagerProxy::reload).
    fn reexecute(&self) -> zbus::Result<()>;

    /// Reload all unit files.
    fn reload(&self) -> zbus::Result<()>;

    /// Reload a unit if it supports it, otherwise restart.
    /// Fails on a service that is not already running.
    #[dbus_proxy(object = "Unit")]
    fn reload_or_restart_unit(&self, name: &UnitName, mode: &StartMode);

    /// Reload a unit if it supports it, otherwise restart.
    /// No-op if service is not already running.
    #[dbus_proxy(object = "Unit")]
    fn reload_or_try_restart_unit(&self, name: &UnitName, mode: &StartMode);

    /// Reload if the unit is already running, otherwise fail.
    #[dbus_proxy(object = "Unit")]
    fn reload_unit(&self, name: &UnitName, mode: &StartMode);

    /// Reset the "failed" state of all units.
    fn reset_failed(&self) -> zbus::Result<()>;

    /// Resets the "failed" state of a specific unit.
    fn reset_failed_unit(&self, name: &UnitName) -> zbus::Result<()>;

    /// Restart a unit if it is running, otherwise fail.
    #[dbus_proxy(object = "Unit")]
    fn restart_unit(&self, name: &UnitName, mode: &StartMode);

    /// Change the default.target link. See bootup(7) for more information.
    fn set_default_target(&self, name: &UnitName, force: bool)
    -> zbus::Result<Vec<UnitFileChange>>;

    /// Alter the environment block that is passed to all spawned processes.
    /// Any previously set environment variables will be overridden.
    fn set_environment(&self, assignments: &Environment) -> zbus::Result<()>;

    /// May be used to modify certain unit properties at runtime. Not all
    /// properties may be changed at runtime, but many resource management
    /// settings (primarily those listed in systemd.resource-control(5)) may.
    /// The changes are applied instantly and stored on disk for future boots,
    /// unless runtime is true, in which case the settings only apply until the
    /// next reboot. name is the name of the unit to modify. Properties are the
    /// settings to set, encoded as an array of property name and value pairs.
    /// Note that this is not a dictionary!  Also note that when setting array
    /// properties with this method usually results in appending to the
    /// pre-configured array. To reset the configured arrays, set the property
    /// to an empty array first and then append to it.
    fn set_unit_properties(
        &self,
        name: &UnitName,
        runtime: bool,
        properties: &[(&str, zbus::zvariant::Value<'_>)],
    ) -> zbus::Result<()>;

    /// Create and start a transient unit which will be released as soon as it
    /// is finished running, not referenced anymore (see
    /// [ref_unit)](ManagerProxy::ref_unit) or the system is rebooted.
    /// `aux` is currently unused and should be passed as an empty array. See
    /// the [New Control Group
    /// Interface](http://www.freedesktop.org/wiki/Software/systemd/ControlGroupInterface/)
    /// for more information how to make use of this functionality for resource
    /// control purposes.
    fn start_transient_unit(
        &self,
        name: &UnitName,
        mode: &StartMode,
        properties: &[(&str, zbus::zvariant::Value<'_>)],
        aux: &[(&str, &[(&str, zbus::zvariant::Value<'_>)])],
    ) -> zbus::Result<TypedObjectPath<JobProxy<'_>>>;

    /// Increment unit reference count.
    fn ref_unit(&self, name: &UnitName) -> zbus::Result<()>;

    /// Decrement unit reference count.
    fn unref_unit(&self, name: &UnitName) -> zbus::Result<()>;

    /// Enqueue a start job and possibly depending jobs.
    #[dbus_proxy(object = "Job")]
    fn start_unit(&self, name: &UnitName, mode: &StartMode);

    /// Similar to [start_unit](ManagerProxy::start_unit) but replaces a job
    /// that is queued for one unit by a job for another unit.
    #[dbus_proxy(object = "Job")]
    fn start_unit_replace(&self, old_unit: &UnitName, new_unit: &UnitName, mode: &StartMode);

    /// Stops the specified unit.
    /// Note that the "isolate" mode is invalid for this method.
    #[dbus_proxy(object = "Job")]
    fn stop_unit(&self, name: &UnitName, mode: &StartMode);

    /// Enable most bus signals to be sent out. Clients which are interested in
    /// signals need to call this method. Signals are only sent out if at least
    /// one client invoked this method.
    /// See [unsubscribe](ManagerProxy::unsubscribe) for unsubscription / closing semantics.
    fn subscribe(&self) -> zbus::Result<()>;

    /// Transition to a new root directory. This is intended to be used by
    /// initial RAM disks. The method takes two arguments: the new root
    /// directory (which needs to be specified) and an init binary path (which
    /// may be left empty, in which case it is automatically searched for). The
    /// state of the system manager will be serialized before the transition.
    /// After the transition, the manager binary on the main system is invoked
    /// and replaces the old PID 1. All state will then be deserialized.
    fn switch_root(&self, new_root: &FilePath, init: &FilePath) -> zbus::Result<()>;

    /// Restart a unit if it's running, otherwise do nothing.
    #[dbus_proxy(object = "Job")]
    fn try_restart_unit(&self, name: &UnitName, mode: &StartMode);

    /// Unmask unit files.
    /// See [enable_unit_files](ManagerProxy::enable_unit_files) for a
    /// description of the boolean flags.
    fn unmask_unit_files(
        &self,
        files: &[&FilePath],
        runtime: bool,
    ) -> zbus::Result<Vec<UnitFileChange>>;

    /// Combination of [unset_environment](ManagerProxy::unset_environment) and
    /// [set_environment](ManagerProxy::set_environment).  It takes two lists.
    /// The first list contains variables to unset, the second one contains
    /// assignments to set.  If a variable is listed in both, the variable is
    /// set after this method returns, i.e. the set list overrides the unset
    /// list.
    fn unset_and_set_environment(
        &self,
        names: &[&str],
        assignments: &Environment,
    ) -> zbus::Result<()>;

    /// Unset environment variables. It takes a string array of environment
    /// variable names. All variables specified will be unset (if they have been
    /// set previously) and no longer be passed to all spawned processes. This
    /// method has no effect for variables that were previously not set, but
    /// will not fail in that case.
    fn unset_environment(&self, names: &[&str]) -> zbus::Result<()>;

    /// Reverts the signal subscription that
    /// [subscribe](ManagerProxy::subscribe) sets up.  It is not usually
    /// necessary to invoke [unsubscribe](ManagerProxy::unsubscribe) as clients
    /// are tracked. Signals are no longer sent out as soon as all clients which
    /// previously asked for [subscribe](ManagerProxy::subscribe) either closed
    /// their connection to the bus or invoked
    /// [unsubscribe](ManagerProxy::unsubscribe).
    fn unsubscribe(&self) -> zbus::Result<()>;

    /// Sent out each time a new job is enqueued or dequeued. Includes the
    /// numeric job ID, the bus path and the primary unit name for this job as
    /// arguments.
    #[dbus_proxy(signal)]
    fn job_new(
        &self,
        id: JobId,
        job: TypedObjectPath<JobProxy<'_>>,
        unit: UnitName,
    ) -> zbus::Result<()>;

    /// Similar to [job_new](ManagerProxy::job_new), but also includes the
    /// [JobResult] of the finished job.
    #[dbus_proxy(signal)]
    fn job_removed(
        &self,
        id: JobId,
        job: TypedObjectPath<JobProxy<'_>>,
        unit: UnitName,
        result: JobResult,
    ) -> zbus::Result<()>;

    /// Sent out immediately before a daemon reload is done (with the boolean
    /// parameter set to True) and after a daemon reload is completed (with the
    /// boolean parameter set to False). This may be used by UIs to optimize UI
    /// updates.
    #[dbus_proxy(signal)]
    fn reloading(&self, active: bool) -> zbus::Result<()>;

    /// Sent out when startup finishes. It carries six microsecond timespan
    /// values, each indicating how much boot time has been spent in each layer
    /// (firmware, bootloader, kernel, initrd) if known, and the always-known
    /// timestamps in the userspace layer, and the total time.
    /// These values may also be calculated from the
    /// [firmware_timestamp_monotonic](ManagerProxy::firmware_timestamp_monotonic),
    /// [loader_timestamp_monotonic](ManagerProxy::loader_timestamp_monotonic),
    /// [initrd_timestamp_monotonic](ManagerProxy::initrd_timestamp_monotonic),
    /// [userspace_timestamp_monotonic](ManagerProxy::userspace_timestamp_monotonic),
    /// and
    /// [finish_timestamp_monotonic](ManagerProxy::finish_timestamp_monotonic)
    /// properties if the system is already ready without the complexity of
    /// handling a signal.
    #[dbus_proxy(signal)]
    fn startup_finished(
        &self,
        firmware: MonotonicTimestamp,
        loader: MonotonicTimestamp,
        kernel: MonotonicTimestamp,
        initrd: MonotonicTimestamp,
        userspace: MonotonicTimestamp,
        total: MonotonicTimestamp,
    ) -> zbus::Result<()>;

    /// Sent out each time the list of enabled or masked unit files on disk have
    /// changed.
    #[dbus_proxy(signal)]
    fn unit_files_changed(&self) -> zbus::Result<()>;

    /// Sent out each time a new unit is loaded or unloaded.
    /// Note that this has little to do with whether a unit is available on disk
    /// or not, and simply reflects the units that are currently loaded into
    /// memory.
    #[dbus_proxy(signal)]
    fn unit_new(&self, id: UnitName, unit: TypedObjectPath<UnitProxy<'_>>) -> zbus::Result<()>;

    /// Sent out each time a new unit is unloaded.
    /// See [unit_new](ManagerProxy::unit_new)
    #[dbus_proxy(signal)]
    fn unit_removed(&self, id: UnitName, unit: TypedObjectPath<UnitProxy<'_>>) -> zbus::Result<()>;

    /// Short ID string describing the architecture the systemd instance is
    /// running on. This follows the same vocabulary as ConditionArchitectures=.
    #[dbus_proxy(property)]
    fn architecture(&self) -> zbus::Result<String>;

    /// Environment property
    #[dbus_proxy(property)]
    fn environment(&self) -> zbus::Result<Environment>;

    /// Features encodes the features that have been enabled and disabled for
    /// this build. Enabled options are prefixed with +, disabled options with
    /// -.
    #[dbus_proxy(property)]
    fn features(&self) -> zbus::Result<String>;

    #[dbus_proxy(property)]
    fn system_state(&self) -> zbus::Result<SystemState>;

    /// Encodes a couple of taint flags as a colon-separated list. When systemd
    /// detects it is running on a system with certain problems, it will set an
    /// appropriate taint flag. Taints may be used to lower the chance of bogus
    /// bug reports.
    #[dbus_proxy(property)]
    fn tainted(&self) -> zbus::Result<TaintSet>;

    /// Version string of the running systemd instance. Note that the version
    /// string is purely informational. It should not be parsed and one may not
    /// assume the version to be formatted in any particular way. We take the
    /// liberty to change the versioning scheme at any time and it is not part
    /// of the public API.
    #[dbus_proxy(property)]
    fn version(&self) -> zbus::Result<String>;

    /// Short ID string describing the virtualization technology the system runs
    /// in. Note that only the "innermost" virtualization technology is exported
    /// here. This detects both full-machine virtualizations (VMs) and
    /// shared-kernel virtualization (containers).
    #[dbus_proxy(property)]
    fn virtualization(&self) -> zbus::Result<Virtualization>;

    #[dbus_proxy(property)]
    fn firmware_timestamp_monotonic(&self) -> zbus::Result<MonotonicTimestamp>;
    #[dbus_proxy(property)]
    fn firmware_timestamp(&self) -> zbus::Result<Timestamp>;

    #[dbus_proxy(property)]
    fn loader_timestamp_monotonic(&self) -> zbus::Result<MonotonicTimestamp>;
    #[dbus_proxy(property)]
    fn loader_timestamp(&self) -> zbus::Result<Timestamp>;

    #[dbus_proxy(property)]
    fn kernel_timestamp_monotonic(&self) -> zbus::Result<MonotonicTimestamp>;
    #[dbus_proxy(property)]
    fn kernel_timestamp(&self) -> zbus::Result<Timestamp>;

    #[dbus_proxy(property, name = "InitRDTimestampMonotonic")]
    fn initrd_timestamp_monotonic(&self) -> zbus::Result<MonotonicTimestamp>;
    #[dbus_proxy(property, name = "InitRDTimestamp")]
    fn initrd_timestamp(&self) -> zbus::Result<Timestamp>;

    #[dbus_proxy(property)]
    fn userspace_timestamp_monotonic(&self) -> zbus::Result<MonotonicTimestamp>;
    #[dbus_proxy(property)]
    fn userspace_timestamp(&self) -> zbus::Result<Timestamp>;

    #[dbus_proxy(property)]
    fn finished_timestamp_monotonic(&self) -> zbus::Result<MonotonicTimestamp>;
    #[dbus_proxy(property)]
    fn finished_timestamp(&self) -> zbus::Result<Timestamp>;
}

#[dbus_proxy(
    interface = "org.freedesktop.systemd1.Job",
    default_service = "org.freedesktop.systemd1",
    gen_blocking = false
)]
trait Job {
    /// See [cancel_job](ManagerProxy::cancel_job)
    fn cancel(&self) -> zbus::Result<()>;

    /// Numeric ID of the job.
    /// During the runtime of a systemd instance each numeric ID is only
    /// assigned once.
    #[dbus_proxy(property)]
    fn id(&self) -> zbus::Result<JobId>;

    /// Unit this job belongs to.
    #[dbus_proxy(property)]
    fn unit(&self) -> zbus::Result<TypedObjectPath<UnitProxy<'_>>>;

    #[dbus_proxy(property)]
    fn job_type(&self) -> zbus::Result<JobType>;

    #[dbus_proxy(property)]
    fn state(&self) -> zbus::Result<JobState>;
}

#[dbus_proxy(
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1",
    gen_blocking = false
)]
trait Unit {
    /// See [start_unit](ManagerProxy::start_unit)
    fn start(&self) -> zbus::Result<()>;

    /// See [stop_unit](ManagerProxy::stop_unit)
    fn stop(&self) -> zbus::Result<()>;

    /// See [reload_unit](ManagerProxy::reload_unit)
    fn reload(&self) -> zbus::Result<()>;

    /// See [restart_unit](ManagerProxy::restart_unit)
    fn restart(&self) -> zbus::Result<()>;

    /// See [try_restart_unit](ManagerProxy::try_restart_unit)
    fn try_restart(&self) -> zbus::Result<()>;

    /// See [reload_or_restart_unit](ManagerProxy::reload_or_restart_unit)
    fn reload_or_restart(&self) -> zbus::Result<()>;

    /// See [reload_or_try_restart_unit](ManagerProxy::reload_or_try_restart_unit)
    fn reload_or_try_restart(&self) -> zbus::Result<()>;

    /// See [kill_unit](ManagerProxy::kill_unit)
    fn kill(&self) -> zbus::Result<()>;

    /// See [reset_failed_unit](ManagerProxy::reset_failed_unit)
    fn reset_failed(&self) -> zbus::Result<()>;

    /// See [ref_unit](ManagerProxy::ref_unit)
    fn incref(&self) -> zbus::Result<()>;

    /// See [unref_unit](ManagerProxy::unref_unit)
    fn unref(&self) -> zbus::Result<()>;

    /// See [set_unit_properties](ManagerProxy::set_unit_properties)
    fn set_properties(
        &self,
        runtime: bool,
        properties: &[(&str, zbus::zvariant::Value<'_>)],
    ) -> zbus::Result<()>;

    /// Primary name of the unit.
    #[dbus_proxy(property)]
    fn id(&self) -> zbus::Result<UnitName>;

    /// All names of the unit, including the primary name that is also exposed
    /// in [id](UnitProxy::id).
    #[dbus_proxy(property)]
    fn names(&self) -> zbus::Result<Vec<UnitName>>;

    /// Human readable description string for the unit.
    #[dbus_proxy(property)]
    fn description(&self) -> zbus::Result<String>;

    /// Reflects whether the configuration file of this unit has been loaded.
    #[dbus_proxy(property)]
    fn load_state(&self) -> zbus::Result<LoadState>;

    /// If the unit failed to load (as encoded in
    /// [load_state)](UnitProxy::load_state), then this will include a D-Bus
    /// error pair consisting of the error ID and an explanatory human readable
    /// string of what happened. If it loaded successfully, this will be a pair
    /// of empty strings.
    #[dbus_proxy(property)]
    fn load_error(&self) -> zbus::Result<(String, String)>;

    /// Reflects whether the unit is currently active or not
    #[dbus_proxy(property)]
    fn active_state(&self) -> zbus::Result<ActiveState>;

    /// Follows states of the same state machine that [ActiveState] covers, but
    /// knows more fine-grained states that are unit-type-specific
    #[dbus_proxy(property)]
    fn sub_state(&self) -> zbus::Result<ActiveState>;

    #[dbus_proxy(property)]
    fn markers(&self) -> zbus::Result<Vec<Marker>>;
}

#[dbus_proxy(
    interface = "org.freedesktop.systemd1.Service",
    default_service = "org.freedesktop.systemd1",
    gen_blocking = false
)]
trait Service {
    #[dbus_proxy(property)]
    fn result(&self) -> zbus::Result<ServiceResult>;

    #[dbus_proxy(property)]
    fn environment(&self) -> zbus::Result<Environment>;

    #[dbus_proxy(property)]
    fn user(&self) -> zbus::Result<String>;

    #[dbus_proxy(property)]
    fn group(&self) -> zbus::Result<String>;

    #[dbus_proxy(property, name = "MainPID")]
    fn main_pid(&self) -> zbus::Result<u32>;
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use byteorder::LE;
    use maplit::btreeset;
    use zvariant::from_slice;
    use zvariant::to_bytes;
    use zvariant::EncodingContext as Context;

    use super::ActiveState;
    use super::EnableDisableUnitFlags;
    use super::KillWhom;
    use super::LoadState;
    use super::Taint;
    use super::TaintSet;
    use super::UnitFileState;
    use super::Virtualization;
    use crate::Systemd;

    #[containertest]
    async fn test_virtualization() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log).await?;
        let virt = sd.virtualization().await?;
        assert_eq!(virt, Virtualization::Container("systemd-nspawn"));
        Ok(())
    }

    #[containertest]
    async fn test_list_units() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log).await?;
        let units = sd.list_units().await?;
        assert!(!units.is_empty());
        let root = units.iter().find(|u| u.name == "-.mount").unwrap();
        assert_eq!(root.active_state, ActiveState::Active);
        assert_eq!(root.load_state, LoadState::Loaded);
        assert_eq!(root.sub_state, "mounted");
        Ok(())
    }

    #[test]
    async fn test_misc_serde() -> Result<()> {
        let ctxt = Context::<LE>::new_dbus(0);
        let encoded = to_bytes(ctxt, "inactive")?;
        let decoded: ActiveState = from_slice(&encoded, ctxt)?;
        assert_eq!(decoded, ActiveState::Inactive);

        let encoded = to_bytes(ctxt, "enabled-runtime")?;
        let decoded: UnitFileState = from_slice(&encoded, ctxt)?;
        assert_eq!(decoded, UnitFileState::EnabledRuntime);
        let encoded = to_bytes(ctxt, "some-other-unknown")?;
        let decoded: UnitFileState = from_slice(&encoded, ctxt)?;
        assert_eq!(
            decoded,
            UnitFileState::Unknown("some-other-unknown".to_string())
        );

        let encoded = to_bytes(
            ctxt,
            &(EnableDisableUnitFlags::RUNTIME | EnableDisableUnitFlags::FORCE),
        )?;
        let decoded: u64 = from_slice(&encoded, ctxt)?;
        assert_eq!(decoded, 3);

        let encoded = to_bytes(ctxt, &KillWhom::Control)?;
        let decoded: String = from_slice(&encoded, ctxt)?;
        assert_eq!(decoded, "control");

        let encoded = to_bytes(ctxt, "mtab-not-symlink:cgroups-missing")?;
        let decoded: TaintSet = from_slice(&encoded, ctxt)?;
        assert_eq!(
            decoded,
            TaintSet(btreeset! {Taint::MtabNotSymlink, Taint::CgroupsMissing})
        );

        Ok(())
    }
}
