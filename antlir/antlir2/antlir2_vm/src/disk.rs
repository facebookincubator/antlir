/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;
use std::process::Command;

use derive_builder::Builder;
use thiserror::Error;
use tracing::debug;

use crate::runtime::get_runtime;
use crate::types::QCow2DiskOpts;
use crate::utils::log_command;

/// A writable QCow2Disk.
/// This create a qcow2 disk on top of the base image file and is passed to qemu with
/// -blockdev and -device.
#[derive(Debug, Builder)]
#[builder(build_fn(name = "build_internal"))]
pub(crate) struct QCow2Disk {
    /// Disk property specified by clients
    opts: QCow2DiskOpts,
    /// Unique name of this -blockdev
    name: String,
    /// State directory
    state_dir: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum QCow2DiskError {
    #[error(transparent)]
    BuilderError(#[from] QCow2DiskBuilderError),
    #[error("qemu-img failed to create the disk: {0}")]
    DiskCreationError(std::io::Error),
    #[error("qemu-img failed to upsize the disk: {0}")]
    DiskUpsizeError(std::io::Error),
}

impl QCow2DiskBuilder {
    // Create and track the temp disk before expose QCow2Disk for use
    pub(crate) fn build(&self) -> Result<QCow2Disk, QCow2DiskError> {
        let mut disk = self.build_internal()?;
        disk.create_temp_disk()?;
        Ok(disk)
    }
}

impl QCow2Disk {
    /// Create a temporary disk with qemu-img inside state directory.
    fn create_temp_disk(&mut self) -> Result<(), QCow2DiskError> {
        let mut cmd = Command::new(&get_runtime().qemu_img);
        cmd.arg("create")
            .arg("-f")
            .arg("qcow2")
            .arg(self.disk_file_name().as_os_str())
            .arg("-F")
            .arg("raw");
        if let Some(image) = &self.opts.base_image {
            cmd.arg("-b").arg(image);
        }
        log_command(&mut cmd)
            .status()
            .map_err(QCow2DiskError::DiskCreationError)?;

        if let Some(size) = self.opts.additional_mib {
            log_command(
                Command::new(&get_runtime().qemu_img)
                    .arg("resize")
                    .arg(self.disk_file_name().as_os_str())
                    .arg(&format!("+{}M", size)),
            )
            .status()
            .map_err(QCow2DiskError::DiskUpsizeError)?;
        }

        debug!(
            "Created {} for {}",
            self.disk_file_name().display(),
            self.name
        );
        Ok(())
    }

    fn disk_file_name(&self) -> PathBuf {
        self.state_dir.join(format!("{}.qcow2", self.name))
    }

    pub(crate) fn qemu_args(&self) -> Vec<String> {
        [
            "-blockdev",
            &format!(
                "driver=qcow2,node-name={},file.driver=file,file.filename={}",
                self.name,
                self.disk_file_name().to_str().expect("Invalid filename"),
            ),
            "-device",
            &format!(
                "{driver},drive={name},serial={name},physical_block_size={pbs},logical_block_size={lbs}",
                driver = self.opts.interface,
                name = self.name,
                pbs = self.opts.physical_block_size,
                lbs = self.opts.logical_block_size,
            ),
        ]
            .iter()
            .map(|x| x.to_string())
            .collect()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_qcow2disk() {
        let opts = QCow2DiskOpts {
            base_image: None,
            additional_mib: None,
            interface: "virtio-blk".to_string(),
            physical_block_size: 512,
            logical_block_size: 512,
        };

        let mut builder = QCow2DiskBuilder::default();
        builder
            .opts(opts)
            .name("test-device".to_string())
            .state_dir(PathBuf::from("/tmp/test"));
        // Can't easily test anything that depends on qemu binaries, so we invoke
        // the internal builder to skip creating the real disk file.
        let disk = builder.build_internal().expect("Failed to build QCow2Disk");

        assert_eq!(
            disk.disk_file_name(),
            PathBuf::from("/tmp/test/test-device.qcow2")
        );
        assert_eq!(
            &disk.qemu_args().join(" "),
            "-blockdev \
            driver=qcow2,node-name=test-device,file.driver=file,file.filename=/tmp/test/test-device.qcow2 \
            -device virtio-blk,drive=test-device,serial=test-device,\
            physical_block_size=512,logical_block_size=512"
        );
    }
}
