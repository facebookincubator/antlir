/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
use metalos_host_configs::runtime_config::Service;
use nix::unistd::chown;
use nix::unistd::Group;
use nix::unistd::User;
use slog::o;
use slog::trace;
use slog::Logger;
use systemd::Marker;
use systemd::StartMode;
use systemd::Systemd;
use systemd::TypedObjectPath;
use systemd::UnitName;
use thrift_wrapper::ThriftWrapper;
use uuid::Uuid;

mod dropin;
mod generator;
mod set;
mod unit_file;
use dropin::Dropin;
use set::ServiceDiff;
pub use set::ServiceSet;

#[cfg(facebook)]
pub mod facebook;

fn systemd_run_unit_path() -> &'static Path {
    Path::new("/run/systemd/system")
}

/// Run details for a single execution of a Native Service.
#[derive(Debug, Clone, ThriftWrapper)]
#[thrift(service_state::types::ServiceInstance)]
pub struct ServiceInstance {
    svc: Service,
    run_uuid: Uuid,
}

impl ServiceInstance {
    pub fn new(service: Service) -> Self {
        let run_uuid = Uuid::new_v4();
        Self {
            svc: service,
            run_uuid,
        }
    }

    fn unique_key(&self) -> String {
        format!(
            "{}-{}-{}",
            self.svc.name(),
            self.svc.svc.id.to_simple(),
            self.run_uuid.to_simple()
        )
    }

    pub fn paths(&self) -> Paths {
        let unique = self.unique_key();
        Paths {
            root_source: self.svc.svc.path(),
            root: metalos_paths::runtime::service_roots().join(&unique),
            state: metalos_paths::runtime::state().join(self.svc.name()),
            cache: metalos_paths::runtime::cache().join(self.svc.name()),
            logs: metalos_paths::runtime::logs().join(self.svc.name()),
            runtime: metalos_paths::runtime::runtime().join(&unique),
        }
    }

    pub fn name(&self) -> &str {
        &self.svc.svc.name
    }

    pub fn version(&self) -> Uuid {
        self.svc.svc.id
    }

    pub fn run_uuid(&self) -> Uuid {
        self.run_uuid
    }

    pub fn unit_name(&self) -> UnitName {
        self.svc.unit_name()
    }

    fn metalos_dir(&self) -> PathBuf {
        self.svc
            .metalos_dir()
            .expect("never None, and expect removed in following diff")
    }

    fn linked_unit_path(&self) -> PathBuf {
        systemd_run_unit_path().join(self.unit_name())
    }

    fn generator_path(&self) -> PathBuf {
        self.metalos_dir().join("generator")
    }

    /// Load the structured service definition that is installed in the image
    pub fn load_shape(&self) -> Result<service_shape::service_t> {
        let path = self.metalos_dir().join("service.shape");
        let buf =
            std::fs::read(&path).with_context(|| format!("while reading {}", path.display()))?;
        fbthrift::binary_protocol::deserialize(&buf)
            .with_context(|| format!("while parsing {}", path.display()))
    }

    /// Makes sure to assign proper ownership to the cache/logs/state directories.
    /// This is needed if the .service file has User/Group directives.
    pub fn set_paths_onwership(&self) -> Result<()> {
        let svc = self
            .load_shape()
            .context("while loading shape to determine owner user")?;
        let uid = User::from_name(&svc.exec_info.runas.user)
            .with_context(|| format!("while looking up user '{}'", svc.exec_info.runas.user))?
            .with_context(|| format!("user '{}' not found", svc.exec_info.runas.user))?
            .uid;
        let gid = Group::from_name(&svc.exec_info.runas.user)
            .with_context(|| format!("while looking up group '{}'", svc.exec_info.runas.group))?
            .with_context(|| format!("group '{}' not found", svc.exec_info.runas.group))?
            .gid;
        chown(self.paths().cache(), Some(uid), Some(gid))?;
        chown(self.paths().logs(), Some(uid), Some(gid))?;
        chown(self.paths().state(), Some(uid), Some(gid))?;
        chown(self.paths().runtime(), Some(uid), Some(gid))?;
        Ok(())
    }

    /// Prepare this service version to be run the next time this service is
    /// restarted. This method will not start the service, but it will ensure
    /// that the MetalOS drop-ins are applied and any service config generator
    /// is executed.
    pub(crate) async fn prepare(self) -> Result<PreparedService> {
        let dropin = Dropin::new(&self)
            .with_context(|| format!("while building dropin for {}", self.svc.name()))?;

        let dropin_dir = Path::new("/run/systemd/system").join(format!("{}.d", &self.unit_name()));
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
                let dest = dropin_dir.join(file_name);
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

        let svc = self.load_shape().context("while loading service shape")?;
        let unit_file: unit_file::UnitFile = svc
            .try_into()
            .context("while converting shape to unit file")?;
        let unit_contents = serde_systemd::to_string(&unit_file)
            .context("while serializing shape to systemd unit")?;
        std::fs::write(self.linked_unit_path(), unit_contents).with_context(|| {
            format!(
                "while writing unit file '{}'",
                self.linked_unit_path().display()
            )
        })?;

        Ok(PreparedService(self))
    }
}

#[derive(Debug, Clone)]
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
        for diff in diff.iter() {
            match diff {
                ServiceDiff::Swap { current, next } => {
                    trace!(log, "preparing to swap {:?}->{:?}", current, next);
                    let svc = ServiceInstance::new(next.clone());
                    to_restart.push(svc.prepare().await.with_context(|| {
                        format!("while preparing to move from {:?} -> {:?}", current, next)
                    })?);
                }
                ServiceDiff::Start(next) => {
                    trace!(log, "preparing to start {:?}", next);
                    let svc = ServiceInstance::new(next.clone());
                    to_start.push(
                        svc.prepare()
                            .await
                            .with_context(|| format!("while preparing to start {:?}", next))?,
                    );
                }
                ServiceDiff::Stop(current) => {
                    trace!(log, "preparing to stop {:?}", current);
                    to_stop.push(current.unit_name());
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
                .get_unit(&svc.unit_name())
                .await
                .with_context(|| format!("while getting unit proxy for {}", svc.name()))?;
            unit.set_properties(
                true,
                &[("Markers", vec![Marker::NeedsRestart.to_string()].into())],
            )
            .await
            .with_context(|| format!("while setting Markers=needs-restart on {}", svc.name()))?;
        }
        let mut jobs = sd
            .enqueue_marked_jobs()
            .await
            .context("while enqueing marked jobs")?;

        trace!(log, "restart jobs = {:?}", jobs);

        // now start all the new services
        for svc in to_start {
            let job = sd
                .start_unit(&svc.unit_name(), &StartMode::Replace)
                .await
                .with_context(|| format!("while starting {}", svc.name()))?;
            trace!(log, "start {}: {}", svc.name(), job.path());
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

            // this is not strictly necessary, delete the service units and any
            // drop-ins for this service to avoid cluttering /run/systemd/system
            for unit in &to_stop {
                let dropin_dir = systemd_run_unit_path().join(format!("{}.d", &unit));
                std::fs::remove_dir_all(&dropin_dir)
                    .with_context(|| format!("while deleting {}", dropin_dir.display()))?;
                std::fs::remove_file(systemd_run_unit_path().join(&unit)).with_context(|| {
                    format!(
                        "while removing linked unit file {}",
                        systemd_run_unit_path().join(unit).display()
                    )
                })?;
            }

            // daemon-reload again to pick up any deleted unit files
            sd.reload().await.context("while doing daemon-reload")?;
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
    use std::fs;
    use std::os::linux::fs::MetadataExt;

    use metalos_host_configs::packages::Format;
    use metalos_host_configs::packages::Service as ServicePackage;
    use metalos_macros::containertest;

    use super::*;

    // In the near future we probably want to assert that the running state of
    // the system matches what we expect before/during/after transactions, but
    // for now let's not do that and only check versions during test
    async fn running_service_version(sd: &Systemd, service: &str) -> Result<String> {
        let set = ServiceSet::current(sd).await?;
        set.iter()
            .find(|s| s.name() == service)
            .with_context(|| format!("{} was not discovered", service))
            .map(|svc| svc.svc.id.to_simple().to_string())
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
            current: ServiceSet::new(vec![]),
            next: ServiceSet::new(vec![Service {
                svc: ServicePackage::new(
                    "metalos.service.demo".into(),
                    "00000000000040008000000000000001"
                        .parse()
                        .expect("this is a valid uuid"),
                    None,
                    Format::Sendstream,
                ),
                config_generator: None,
            }]),
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
            ServiceSet::new(vec![Service {
                svc: ServicePackage::new(
                    "metalos.service.demo".into(),
                    "00000000000040008000000000000001"
                        .parse()
                        .expect("this is a valid uuid"),
                    None,
                    Format::Sendstream,
                ),
                config_generator: None,
            }]),
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
            ServiceSet::new(vec![Service {
                svc: ServicePackage::new(
                    "metalos.service.demo".into(),
                    "00000000000040008000000000000002"
                        .parse()
                        .expect("this is a valid uuid"),
                    None,
                    Format::Sendstream,
                ),
                config_generator: None,
            }]),
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
            ServiceSet::new(vec![Service {
                svc: ServicePackage::new(
                    "metalos.service.demo".into(),
                    "00000000000040008000000000000001"
                        .parse()
                        .expect("this is a valid uuid"),
                    None,
                    Format::Sendstream,
                ),
                config_generator: None,
            }]),
        )
        .await?
        .commit(log.clone(), &sd)
        .await?;

        assert_eq!(
            running_service_version(&sd, "metalos.service.demo").await?,
            "00000000000040008000000000000001",
        );

        Transaction::with_next(&sd, ServiceSet::new(vec![]))
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

    #[containertest]
    async fn unit_file() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let sd = Systemd::connect(log.clone()).await?;

        Transaction {
            current: ServiceSet::new(vec![]),
            next: ServiceSet::new(vec![Service {
                svc: ServicePackage::new(
                    "metalos.service.demo".into(),
                    "00000000000040008000000000000001"
                        .parse()
                        .expect("this is a valid uuid"),
                    None,
                    Format::Sendstream,
                ),
                config_generator: None,
            }]),
        }
        .commit(log.clone(), &sd)
        .await?;

        let path = "/run/systemd/system/metalos.service.demo.service";
        let service_content = std::fs::read_to_string(&path)
            .with_context(|| format!("while reading service file in {}", path))?;

        // NOTE: the extra blank lines found here are not important but is an
        // artifact of serde_systemd's output. If extra linebreaks are removed
        // from serde_systemd they should also be removed from here
        pretty_assertions::assert_eq!(
            "[Unit]\n\
             [Service]\n\
             ExecStart=/bin/bash -c 'sleep 12h'\n\
             \n\
             ExecStartPre=/metalos/bin/metalos.lib.service.tests.demo_service:demo-service '--run=${RUNTIME_DIRECTORY}' '--state=${STATE_DIRECTORY}' '--cache=${CACHE_DIRECTORY}' '--logs=${LOGS_DIRECTORY}'\n\
             \n\
             Group=demoservice\n\
             Type=simple\n\
             User=demoservice\n\
             Environment=FB_SERVICE_ID=wdb/demo-service\n\
             \n",
            service_content
        );
        Ok(())
    }
}
