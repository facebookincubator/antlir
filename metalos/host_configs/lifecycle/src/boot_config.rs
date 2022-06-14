/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{ensure, Context};

use metalos_host_configs::boot_config::BootConfig;
use metalos_host_configs::packages::generic::Package;
use package_download::PackageExt;

use crate::stage::StagableConfig;

impl StagableConfig for BootConfig {
    #[deny(unused_variables)]
    fn packages(&self) -> Vec<Package> {
        let Self {
            #[cfg(facebook)]
                deployment_specific: _,
            rootfs,
            kernel,
            initrd,
            bootloader,
        } = self.clone();
        let mut pkgs = vec![rootfs.into(), kernel.pkg.into(), initrd.into()];
        if let Some(b) = bootloader {
            pkgs.push(b.pkg.into());
        }
        pkgs
    }

    fn check_downloaded_artifacts(&self) -> anyhow::Result<()> {
        ensure!(
            self.rootfs
                .on_disk()
                .context("rootfs not on disk")?
                .path()
                .join("usr/lib/os-release")
                .exists(),
            "rootfs does not look like an os tree"
        );
        ensure!(self.kernel.vmlinuz().is_some(), "missing vmlinuz");
        ensure!(
            self.kernel.disk_boot_modules().is_some(),
            "missing disk boot modules"
        );
        ensure!(self.kernel.modules().is_some(), "missing modules dir");
        Ok(())
    }
}
