/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use nix::unistd::chown;
use nix::unistd::Gid;
use nix::unistd::Group;
use nix::unistd::Uid;
use nix::unistd::User;
use std::collections::HashSet;
use std::fs::read_dir;
use std::fs::OpenOptions;
use std::ops::Deref;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use futures::StreamExt;
use itertools::Itertools;
use serde::Deserialize;
use serde::Serialize;
use slog::o;
use slog::trace;
use slog::Logger;
use uuid::Uuid;

use systemd::EnableDisableUnitFlags;
use systemd::Marker;
use systemd::StartMode;
use systemd::Systemd;
use systemd::TypedObjectPath;
use systemd::UnitName;
use systemd_parser::items::*;

mod dropin;
mod generator;
mod set;
mod unit_file;
use dropin::Dropin;
use set::ServiceDiff;
pub use set::ServiceSet;

#[cfg(facebook)]
pub(crate) mod facebook;

pub type Version = Uuid;

/// Run details for a single execution of a Native Service.
#[derive(Debug, Deserialize, Serialize)]
pub struct ServiceInstance {
    name: String,
    version: Uuid,
    run_uuid: Uuid,
    paths: Paths,
    unit_name: UnitName,
}

impl ServiceInstance {
    pub fn new(name: String, version: Uuid) -> Self {
        let run_uuid = Uuid::new_v4();
        let unique = format!("{}-{}-{}", name, version.to_simple(), run_uuid.to_simple());
        let base = metalos_paths::runtime();
        let paths = Paths {
            root_source: metalos_paths::images().join("service").join(format!(
                "{}:{}",
                name,
                version.to_simple()
            )),
            root: base.join("service-roots").join(&unique),
            state: base.join("state").join(&name),
            cache: base.join("cache").join(&name),
            logs: base.join("logs").join(&name),
            runtime: base.join("runtime").join(unique),
        };
        let unit_name = format!("{}.service", name).into();
        Self {
            name,
            version,
            run_uuid,
            paths,
            unit_name,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> Uuid {
        self.version
    }

    pub fn run_uuid(&self) -> Uuid {
        self.run_uuid
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn unit_name(&self) -> &UnitName {
        &self.unit_name
    }

    fn metalos_dir(&self) -> PathBuf {
        self.paths.root_source.join("metalos")
    }

    fn unit_file_path(&self) -> PathBuf {
        self.metalos_dir().join(&self.unit_name)
    }

    // TODO(T111087410): this should come from a separate package
    fn generator_path(&self) -> PathBuf {
        self.metalos_dir().join("generator")
    }

    /// Makes sure to assign proper ownership to the cache/logs/state directories.
    /// This is needed if the .service file has User/Group directives.
    pub fn set_paths_onwership(&self) -> Result<()> {
        let mut uid = Uid::from_raw(0);
        let mut gid = Gid::from_raw(0);
        let file_content = std::fs::read_to_string(self.unit_file_path()).with_context(|| {
            format!(
                "while reading unit file {}",
                self.unit_file_path().display()
            )
        })?;
        // NOTE: ideally I would use dbus to get the User/Group for a unit, however
        // when this function runs the service might not be loaded....
        // Thefore we have to parse the unit file with the systemd_parser crate.
        let systemd_unit = systemd_parser::parse_string(&file_content)?;
        if let Some(&DirectiveEntry::Solo(ref u)) = systemd_unit.lookup_by_key("User") {
            if let Some(user) = u.value() {
                uid = User::from_name(user)
                    .with_context(|| format!("user {} not found", user))?
                    .with_context(|| format!("can't find uid for user {}", user))?
                    .uid;
            }
        }
        if let Some(&DirectiveEntry::Solo(ref g)) = systemd_unit.lookup_by_key("Group") {
            if let Some(group) = g.value() {
                gid = Group::from_name(group)
                    .with_context(|| format!("group {} not found", group))?
                    .with_context(|| format!("can't find gid for group {}", group))?
                    .gid;
            }
        }
        chown(self.paths().cache(), Some(uid), Some(gid))?;
        chown(self.paths().logs(), Some(uid), Some(gid))?;
        chown(self.paths().state(), Some(uid), Some(gid))?;
        chown(self.paths().runtime(), Some(uid), Some(gid))?;
        Ok(())
    }

    /// Prepare this service version to be run the next time this service is
    /// restarted. This method will not start the service, but it will link it.
    /// A separate daemon-reload must be triggered for systemd to load the new
    /// unit settings.
    pub(crate) async fn prepare(self, sd: &Systemd) -> Result<PreparedService> {
        let dropin = Dropin::new(&self)
            .with_context(|| format!("while building dropin for {}", self.unit_name))?;

        let dropin_dir = Path::new("/run/systemd/system").join(format!("{}.d", &self.unit_name));
        std::fs::create_dir_all(&dropin_dir)
            .with_context(|| format!("while creating dropin dir {}", dropin_dir.display()))?;
        let dropin_file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(dropin_dir.join("99-metalos.conf"))
            .context("while creating dropin file")?;
        serde_systemd::to_writer(dropin_file, &dropin)?;

        // symlink all default dropin found in /usr/lib/metalos/native-service/dropins/<file>
        // into /run/systemd/system/{unit_name}.d/<file>
        let entries = read_dir("/usr/lib/metalos/native-service/dropins")
            .context("Can't find dropins directory for native services")?;
        for entry in entries {
            let entry = entry?;
            if entry.path().is_dir() {
                continue;
            }
            if let Some(file_name) = entry.path().file_name() {
                let dest = Path::new("/run/systemd/system")
                    .join(format!("{}.d/", &self.unit_name))
                    .join(file_name);
                if dest.exists() {
                    std::fs::remove_file(dest.clone()).context(format!(
                        "while deleting existent symlink {:#?}",
                        dest.clone()
                    ))?;
                }
                symlink(entry.path(), dest.clone()).context(format!(
                    "when symlinking dropin {:?} to {:?}",
                    entry.path(),
                    dest
                ))?;
            }
        }

        if self.generator_path().exists() {
            let output =
                crate::generator::evaluate_generator(self.generator_path()).with_context(|| {
                    format!(
                        "while running generator at {}",
                        self.generator_path().display()
                    )
                })?;
            if let Some(dropin) = output.dropin {
                let dropin_path = dropin_dir.join("50-generator.conf");
                let dropin_file = OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&dropin_path)
                    .with_context(|| {
                        format!(
                            "while creating generator dropin file ({})",
                            dropin_path.display()
                        )
                    })?;
                let dropin = crate::generator::GeneratedDropin::from(dropin);
                serde_systemd::to_writer(dropin_file, &dropin).with_context(|| {
                    format!(
                        "while serializing {:?} to generator dropin file ({})",
                        dropin,
                        dropin_path.display()
                    )
                })?;
            }
        }

        let unit_file = self.unit_file_path();
        // overwrite any existing link, since a different version of the service
        // could already be running
        sd.link_unit_files(&[systemd::FilePath::new(&unit_file)], true, true)
            .await
            .with_context(|| format!("while linking {}", unit_file.display()))?;
        Ok(PreparedService(self))
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Paths {
    root_source: PathBuf,
    root: PathBuf,
    state: PathBuf,
    cache: PathBuf,
    logs: PathBuf,
    runtime: PathBuf,
}

impl Paths {
    /// R/O subvolume of the service's root directory.
    pub fn root_source(&self) -> &Path {
        &self.root_source
    }

    /// R/W snapshot of the service's root directory. This directory is valid
    /// for only one service lifecycle and will be deleted as soon as the
    /// service stops.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Persistent on-host storage. Saved across service restarts and never
    /// purged without external intervention.
    pub fn state(&self) -> &Path {
        &self.state
    }

    /// Semi-persistent on-host storage. MetalOS will preserve this on a
    /// best-effort basis, but reserves the right to purge this directory
    /// whenever the service is stopped.
    pub fn cache(&self) -> &Path {
        &self.cache
    }

    /// Semi-persistent on-host storage for text-based log storage. Where
    /// possible, journald is strongly preferred over text-based logs in this
    /// directroy. See also [Paths::cache].
    pub fn logs(&self) -> &Path {
        &self.logs
    }

    /// Volatile storage. This directory is valid for only one service lifecycle
    /// and will be deleted as soon as the service stops.
    pub fn runtime(&self) -> &Path {
        &self.runtime
    }
}

/// Token to prove that a service has linked unit files and written drop-ins to
/// run a specific version.
#[derive(Debug)]
struct PreparedService(ServiceInstance);

impl Deref for PreparedService {
    type Target = ServiceInstance;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// This is possibly named too optimistically because systemd does not actually
/// offer strong, transactional semantics but MetalOS does its best to provide
/// "transactions" on top of native service lifecycles.
#[derive(Debug)]
pub struct Transaction {
    current: ServiceSet,
    next: ServiceSet,
}

impl Transaction {
    /// Create a Transaction that will move the system from the current state to
    /// the next desired state.
    pub async fn with_next(sd: &Systemd, next: ServiceSet) -> Result<Self> {
        let current = ServiceSet::current(sd)
            .await
            .context("while loading the current service set")?;
        Ok(Self { current, next })
    }

    /// Create a Transaction that will move the system from a given state to the
    /// next desired state.
    pub(crate) fn new(current: ServiceSet, next: ServiceSet) -> Self {
        Self { current, next }
    }

    /// Attempt to commit the set of changes required to bring the system from
    /// current_state to next_state.
    /// TODO(T114714686): make this a better Error type that both attempts to
    /// complete as much of the transaction as possible, and collects errors
    /// specific to each service that failed, instead of just failing as one
    /// huge chunk.
    pub async fn commit(self, log: Logger, sd: &Systemd) -> Result<()> {
        let log = log.new(o!(
            "next" => format!("{:?}", self.next),
            "current" => format!("{:?}", self.current),
        ));
        let diff = self.current.diff(&self.next);
        if diff.is_empty() {
            trace!(log, "no service changes to apply");
            return Ok(());
        }
        let mut to_start = vec![];
        let mut to_restart = vec![];
        let mut to_stop = vec![];
        for (name, diff) in diff.iter() {
            match diff {
                ServiceDiff::Swap { current, next } => {
                    trace!(
                        log,
                        "preparing to swap {} {}->{}",
                        name,
                        current.to_simple(),
                        next.to_simple()
                    );
                    let svc = ServiceInstance::new(name.clone(), *next);
                    to_restart.push(svc.prepare(sd).await.with_context(|| {
                        format!(
                            "while preparing to move {} from {} -> {}",
                            name,
                            current.to_simple(),
                            next.to_simple()
                        )
                    })?);
                }
                ServiceDiff::Start(next) => {
                    trace!(log, "preparing to start {}:{}", name, next.to_simple());
                    let svc = ServiceInstance::new(name.clone(), *next);
                    to_start.push(svc.prepare(sd).await.with_context(|| {
                        format!("while preparing to start {}:{}", name, next.to_simple())
                    })?);
                }
                ServiceDiff::Stop(current) => {
                    trace!(log, "preparing to stop {}:{}", name, current.to_simple());
                    let svc = ServiceInstance::new(name.clone(), *current);
                    to_stop.push(svc.unit_name);
                }
            }
        }

        // daemon-reload just once to reload all the new and updated unit settings
        sd.reload().await.context("while doing daemon-reload")?;

        sd.subscribe()
            .await
            .or_else(|e| match e {
                zbus::Error::MethodError(ref name, _, _) => {
                    if name.as_str() == "org.freedesktop.systemd1.AlreadySubscribed" {
                        Ok(())
                    } else {
                        Err(Error::msg(e))
                    }
                }
                _ => Err(Error::msg(e)),
            })
            .context("while subscribing to systemd events")?;
        let mut job_removed_stream = sd
            .receive_job_removed()
            .await
            .context("while creating stream for JobRemoved signals")?;

        // mark all services that need to be restarted and enqueue all the jobs
        // at once
        for svc in &to_restart {
            let unit = sd
                .get_unit(&svc.unit_name)
                .await
                .with_context(|| format!("while getting unit proxy for {}", svc.unit_name))?;
            unit.set_properties(
                true,
                &[("Markers", vec![Marker::NeedsRestart.to_string()].into())],
            )
            .await
            .with_context(|| format!("while setting Markers=needs-restart on {}", svc.unit_name))?;
        }
        let mut jobs = sd
            .enqueue_marked_jobs()
            .await
            .context("while enqueing marked jobs")?;

        trace!(log, "restart jobs = {:?}", jobs);

        // now start all the new services
        for svc in to_start {
            let job = sd
                .start_unit(&svc.unit_name, &StartMode::Replace)
                .await
                .with_context(|| format!("while starting {}", svc.unit_name))?;
            trace!(log, "start {}: {}", svc.unit_name, job.path());
            jobs.push(unsafe { TypedObjectPath::from_untyped(job.path()) });
        }

        // finally, stop any services that were removed
        if !to_stop.is_empty() {
            for unit in &to_stop {
                let job = sd
                    .stop_unit(&unit, &StartMode::Fail)
                    .await
                    .with_context(|| format!("while stopping {}", unit))?;
                trace!(log, "stop {}: {}", unit, job.path());
                jobs.push(unsafe { TypedObjectPath::from_untyped(job.path()) });
            }
            // also unlink all the unit files for stopped services
            let units: Vec<_> = to_stop.iter().collect();
            trace!(log, "unlinking [{}]", units.iter().join(","));
            sd.disable_unit_files(units.as_slice(), EnableDisableUnitFlags::RUNTIME)
                .await
                .context("while unlinking stopped service unit files")?;

            // this is not strictly necessary, but delete any drop-ins for this
            // service to avoid cluttering /run/systemd/system
            for unit in &to_stop {
                let dropin_dir = Path::new("/run/systemd/system").join(format!("{}.d", &unit));
                std::fs::remove_dir_all(&dropin_dir)
                    .with_context(|| format!("while deleting {}", dropin_dir.display()))?;
            }
        }

        let mut jobs: HashSet<_> = jobs.into_iter().collect();

        ensure!(
            !jobs.is_empty(),
            "there are no jobs to wait for, something went wrong applying this diff"
        );

        trace!(
            log,
            "waiting for [{}]",
            jobs.iter().map(|j| j.to_string()).join(",")
        );

        while let Some(signal) = job_removed_stream.next().await {
            let args = signal
                .args()
                .context("while inspecting JobRemoved signal")?;
            // I fought valiantly against the borrow checker to allow me to just
            // .clone() this TypedObjectPath, but I lost
            let job_path: zvariant::OwnedObjectPath = args.job.clone().into();
            let job = unsafe { TypedObjectPath::from_untyped(&job_path.as_ref()) };
            if jobs.remove(&job) {
                trace!(log, "{} ({}) completed", job.to_string(), args.unit);
            }
            if jobs.is_empty() {
                trace!(log, "all jobs are finished");
                break;
            }
        }

        // try to unsubscribe from future signals, but it doesn't matter if this
        // fails
        let _ = sd.unsubscribe().await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metalos_macros::containertest;
    use set::tests::service_set;
    use std::fs;
    use std::os::linux::fs::MetadataExt;
    use systemd::WaitableSystemState;

    pub(crate) async fn wait_for_systemd() -> anyhow::Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log.clone()).await?;
        sd.wait(WaitableSystemState::Operational).await?;
        Ok(())
    }

    // In the near future we probably want to assert that the running state of
    // the system matches what we expect before/during/after transactions, but
    // for now let's not do that and only check versions during test
    async fn running_service_version(sd: &Systemd, service: &str) -> Result<String> {
        let set = ServiceSet::current(sd).await?;
        set.get(service)
            .with_context(|| format!("{} was not discovered", service))
            .map(|uuid| uuid.to_simple().to_string())
    }

    fn check_path_ownership<P>(path: P, owner_username: &str, owner_group: &str) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let metadata = fs::metadata(path)?;
        let uid = metadata.st_uid();
        let gid = metadata.st_gid();
        let owner_uid = User::from_name(owner_username)?.unwrap().uid.as_raw();
        let owner_gid = Group::from_name(owner_group)?.unwrap().gid.as_raw();
        assert_eq!(uid, owner_uid);
        assert_eq!(gid, owner_gid);
        Ok(())
    }

    /// Start the demo service. If this doesn't work something is fundamentally
    /// broken and should be easier to debug than the `lifecycle_dance` test below
    #[containertest]
    async fn start() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log.clone()).await?;

        Transaction {
            current: set::tests::service_set! {},
            next: set::tests::service_set! {
                "metalos.service.demo" => 1,
            },
        }
        .commit(log.clone(), &sd)
        .await?;

        for d in &["state", "cache", "logs"] {
            let path = format!("/run/fs/control/run/{}/metalos.service.demo/version", d);
            let version_log = std::fs::read_to_string(path.clone())
                .with_context(|| format!("while reading version file in {}", path))?;
            assert_eq!("00000000000040008000000000000001\n", version_log);

            check_path_ownership(path, "demoservice", "demoservice")?;
        }

        Ok(())
    }

    /// Take the demo service through all lifecycle transitions: start, upgrade
    /// and stop
    #[containertest]
    async fn lifecycle_dance() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log.clone()).await?;

        Transaction::with_next(
            &sd,
            service_set! {
                "metalos.service.demo" => 1,
            },
        )
        .await?
        .commit(log.clone(), &sd)
        .await?;

        assert_eq!(
            running_service_version(&sd, "metalos.service.demo").await?,
            "00000000000040008000000000000001",
        );

        Transaction::with_next(
            &sd,
            service_set! {
                "metalos.service.demo" => 2,
            },
        )
        .await?
        .commit(log.clone(), &sd)
        .await?;

        assert_eq!(
            running_service_version(&sd, "metalos.service.demo").await?,
            "00000000000040008000000000000002",
        );

        Transaction::with_next(
            &sd,
            service_set! {
                "metalos.service.demo" => 1,
            },
        )
        .await?
        .commit(log.clone(), &sd)
        .await?;

        assert_eq!(
            running_service_version(&sd, "metalos.service.demo").await?,
            "00000000000040008000000000000001",
        );

        Transaction::with_next(&sd, service_set! {})
            .await?
            .commit(log.clone(), &sd)
            .await?;

        // now the service is stopped, this function should fail
        let version = running_service_version(&sd, "metalos.service.demo").await;
        assert!(
            version.is_err(),
            "should not have found a running version: {:?}",
            version
        );

        for d in &["state", "cache", "logs"] {
            let path = format!("/run/fs/control/run/{}/metalos.service.demo/version", d);
            let version_log = std::fs::read_to_string(path.clone())
                .with_context(|| format!("while reading version file in {}", path))?;
            assert_eq!(
                "00000000000040008000000000000001\n00000000000040008000000000000002\n00000000000040008000000000000001\n",
                version_log
            );
            check_path_ownership(path, "demoservice", "demoservice")?;
        }

        Ok(())
    }
}
