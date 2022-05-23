/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use btrfs::{DeleteFlags, SnapshotFlags, Subvolume};
use std::path::Path;
use thiserror::Error;

use service::ServiceInstance;

#[derive(Error, Debug)]
pub enum Error {
    /// There was a problem creating a subvol. The inner [btrfs::Error] provides
    /// enough context to debug this problem.
    #[error(transparent)]
    Create(btrfs::Error),
    /// There was a problem retrieving info about the subvol. The inner
    /// [btrfs::Error] provides enough context to debug this problem.
    #[error(transparent)]
    Get(btrfs::Error),
    /// There was a problem setting up the root subvol. The inner [btrfs::Error]
    /// provides enough context to debug this problem.
    #[error(transparent)]
    RootSetup(btrfs::Error),
    #[error("failed to delete one or more subvols: {0:?}")]
    Delete(Vec<btrfs::Error>),
    #[error("failed to change ownership of one or more subvols: {0:?}")]
    Permissions(anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// See [service::Paths] for the details on all of the MetalOS Native Service
/// subvolumes.
#[derive(Debug)]
pub(crate) struct ServiceVolumes {
    root: Subvolume,
    runtime: Subvolume,
}

impl ServiceVolumes {
    fn ensure_subvol_exists(path: &Path) -> Result<Subvolume> {
        Subvolume::get(path)
            .or_else(|_| Subvolume::create(path))
            .map_err(Error::Create)
    }

    /// Create (or ensure that they have already been created) the subvolumes
    /// required for a specific run of a native service.
    pub(crate) fn create(svc: &ServiceInstance) -> Result<Self> {
        let paths = svc.paths();

        // ensure that the persistent subvolumes exist, creating them if not
        Self::ensure_subvol_exists(paths.state())?;
        Self::ensure_subvol_exists(paths.cache())?;
        Self::ensure_subvol_exists(paths.logs())?;

        // root and runtime are ephemeral for each run of the native service, so
        // create them fresh
        let root_src = Subvolume::get(paths.root_source()).map_err(Error::Get)?;
        let mut root = root_src
            .snapshot(paths.root(), SnapshotFlags::RECURSIVE)
            .map_err(Error::RootSetup)?;
        root.set_readonly(false).map_err(Error::RootSetup)?;

        let runtime = Subvolume::create(paths.runtime()).map_err(Error::Create)?;

        svc.set_paths_onwership().map_err(Error::Permissions)?;

        Ok(Self { root, runtime })
    }

    /// Get the existing set of subvolumes for a native service instance.
    pub(crate) fn get(svc: &ServiceInstance) -> Result<Self> {
        let paths = svc.paths();
        Ok(Self {
            root: Subvolume::get(paths.root()).map_err(Error::Get)?,
            runtime: Subvolume::get(paths.runtime()).map_err(Error::Get)?,
        })
    }

    pub(crate) fn delete(self) -> Result<()> {
        let mut errors = vec![];
        if let Err(e) = self.root.delete(DeleteFlags::RECURSIVE) {
            errors.push(e);
        }
        if let Err(e) = self.runtime.delete(DeleteFlags::RECURSIVE) {
            errors.push(e);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::Delete(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use metalos_macros::containertest;
    use nix::unistd::{Group, User};
    use std::fs;
    use std::os::linux::fs::MetadataExt;
    use std::path::Path;

    fn do_create() -> Result<(ServiceVolumes, ServiceInstance)> {
        let svc = ServiceInstance::new(
            "metalos.service.demo".into(),
            "00000000-0000-4000-8000-000000000001".parse().unwrap(),
        );
        let svc_vols = ServiceVolumes::create(&svc)?;
        Ok((svc_vols, svc))
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

    fn assert_paths(svc_vols: ServiceVolumes, svc: ServiceInstance) -> Result<()> {
        assert_eq!(
            svc_vols.root.path(),
            Path::new(&format!(
                "/run/fs/control/run/service-roots/metalos.service.demo-{}-{}",
                svc.version().to_simple(),
                svc.run_uuid().to_simple(),
            )),
        );
        assert_eq!(
            svc_vols.runtime.path(),
            Path::new(&format!(
                "/run/fs/control/run/runtime/metalos.service.demo-{}-{}",
                svc.version().to_simple(),
                svc.run_uuid().to_simple(),
            )),
        );
        check_path_ownership(
            format!(
                "/run/fs/control/run/runtime/metalos.service.demo-{}-{}",
                svc.version().to_simple(),
                svc.run_uuid().to_simple()
            ),
            "demoservice",
            "demoservice",
        )?;

        // ensure that the other subvols exist
        assert!(Path::new("/run/fs/control/run/state/metalos.service.demo").exists());
        assert!(Path::new("/run/fs/control/run/cache/metalos.service.demo").exists());
        assert!(Path::new("/run/fs/control/run/logs/metalos.service.demo").exists());

        for dir in &["state", "cache", "logs"] {
            check_path_ownership(
                format!("/run/fs/control/run/{}/metalos.service.demo", dir),
                "demoservice",
                "demoservice",
            )?;
        }
        Ok(())
    }

    #[containertest]
    async fn create() -> Result<()> {
        crate::tests::wait_for_systemd().await?;
        let (svc_vols, svc) = do_create()?;
        assert_paths(svc_vols, svc);
        Ok(())
    }

    #[containertest]
    async fn get() -> Result<()> {
        crate::tests::wait_for_systemd().await?;
        let (_, svc) = do_create()?;
        let svc_vols = ServiceVolumes::get(&svc)?;
        assert_paths(svc_vols, svc);
        Ok(())
    }
}
