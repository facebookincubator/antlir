/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use derive_builder::Builder;
use thiserror::Error;
use tracing::debug;

use crate::isolation::IsolationError;
use crate::isolation::Platform;
use crate::pci::PCIBridges;
use crate::runtime::get_runtime;
use crate::types::QCow2DiskOpts;
use crate::types::QemuDevice;
use crate::utils::run_command_capture_output;

/// A writable QCow2Disk.
/// This create a qcow2 disk on top of the base image file and is passed to qemu with
/// -blockdev and -device.
#[derive(Debug, Builder)]
#[builder(build_fn(name = "build_internal"))]
pub(crate) struct QCow2Disk {
    /// Disk property specified by clients
    opts: QCow2DiskOpts,
    /// The bus name to attache the disk to
    bus: String,
    /// Name prefix
    #[builder(default = "\"vd\".to_string()")]
    prefix: String,
    /// Unique id of this -blockdev
    id: usize,
    /// State directory
    state_dir: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum QCow2DiskError {
    #[error(transparent)]
    BuilderError(#[from] QCow2DiskBuilderError),
    #[error(transparent)]
    RepoRootError(#[from] IsolationError),
    #[error("qemu-img failed to create the disk: {0}")]
    DiskCreationError(std::io::Error),
    #[error("qemu-img failed to upsize the disk: {0}")]
    DiskUpsizeError(std::io::Error),
}

type Result<T> = std::result::Result<T, QCow2DiskError>;

impl QCow2DiskBuilder {
    // Create and track the temp disk before expose QCow2Disk for use
    pub(crate) fn build(&self) -> Result<QCow2Disk> {
        let mut disk = self.build_internal()?;
        disk.create_temp_disk()?;
        Ok(disk)
    }
}

impl QCow2Disk {
    /// Create a temporary disk with qemu-img inside state directory.
    fn create_temp_disk(&mut self) -> Result<()> {
        let mut cmd = Command::new(&get_runtime().qemu_img);
        cmd.arg("create")
            .arg("-f")
            .arg("qcow2")
            .arg(self.disk_file_name().as_os_str())
            .arg("-F")
            .arg("raw");
        if let Some(image) = &self.opts.base_image {
            cmd.arg("-b").arg(self.format_image_path(image)?);
        }
        run_command_capture_output(&mut cmd).map_err(QCow2DiskError::DiskCreationError)?;

        if let Some(size) = self.opts.additional_mib {
            if size != 0 {
                let mut cmd = Command::new(&get_runtime().qemu_img);
                cmd.arg("resize")
                    .arg(self.disk_file_name().as_os_str())
                    .arg(&format!("+{}M", size));
                run_command_capture_output(&mut cmd).map_err(QCow2DiskError::DiskUpsizeError)?;
            }
        }

        debug!(
            "Created {} for {}",
            self.disk_file_name().display(),
            self.name()
        );
        Ok(())
    }

    fn name(&self) -> String {
        format!("{}{}", self.prefix, self.id)
    }

    fn disk_file_name(&self) -> PathBuf {
        self.state_dir.join(format!("{}.qcow2", self.name()))
    }

    fn serial(&self) -> String {
        match &self.opts.serial {
            Some(serial) => serial.clone(),
            None => self.name(),
        }
    }

    /// qemu-img has this unfortunate feature that if a relative path is given
    /// for -b, it will be looked up relative to the directory containing the
    /// resulting image file. Override relative path to be absolute with our
    /// repo root, because all base images should be build artifacts relative
    /// to the repo root.
    fn format_image_path(&self, path: &PathBuf) -> Result<PathBuf> {
        if path.is_relative() {
            Ok(Platform::repo_root()?.join(path))
        } else {
            Ok(path.clone())
        }
    }
}

impl QemuDevice for QCow2Disk {
    fn qemu_args(&self) -> Vec<OsString> {
        let mut args = vec![
            "-blockdev".into(),
            format!(
                "driver=qcow2,node-name={},file.driver=file,file.filename={}",
                self.name(),
                self.disk_file_name().to_str().expect("Invalid filename"),
            )
            .into(),
        ];
        let mut bus = self.bus.clone();
        // Create AHCI controller for SATA drives
        if self.opts.interface == "ide-hd" {
            args.push("-device".into());
            args.push(format!("ahci,id=ahci-{},bus={}", self.name(), bus).into());
            bus = format!("ahci-{}.0", self.name());
        }
        args.push("-device".into());
        args.push(format!(
            "{driver},bus={bus},drive={name},serial={serial},physical_block_size={pbs},logical_block_size={lbs}",
            driver = self.opts.interface,
            bus = bus,
            name = self.name(),
            serial = self.serial(),
            pbs = self.opts.physical_block_size,
            lbs = self.opts.logical_block_size,
        ).into());
        args
    }
}

#[derive(Debug)]
pub(crate) struct QCow2Disks(Vec<QCow2Disk>);

impl QCow2Disks {
    pub(crate) fn new(
        opts: &[QCow2DiskOpts],
        pci_bridges: &PCIBridges,
        state_dir: &Path,
    ) -> Result<Self> {
        let disks: Result<Vec<_>> = opts
            .iter()
            .enumerate()
            .map(|(i, x)| -> Result<_> {
                QCow2DiskBuilder::default()
                    .opts(x.clone())
                    .id(i)
                    .bus(pci_bridges.bridge_for_device_id(i).name())
                    .state_dir(state_dir.to_path_buf())
                    .build()
            })
            .collect();
        Ok(Self(disks?))
    }
}

impl QemuDevice for QCow2Disks {
    fn qemu_args(&self) -> Vec<OsString> {
        self.0.iter().flat_map(|x| x.qemu_args()).collect()
    }
}

#[cfg(test)]
mod test {
    use std::ffi::OsStr;

    use super::*;

    fn build_test_qcow2disk(id: usize) -> QCow2Disk {
        let opts = QCow2DiskOpts {
            interface: "virtio-blk".to_string(),
            physical_block_size: 512,
            logical_block_size: 512,
            ..Default::default()
        };

        let mut builder = QCow2DiskBuilder::default();
        builder
            .opts(opts)
            .bus("pci0".to_string())
            .prefix("test-device".to_string())
            .id(id)
            .state_dir(PathBuf::from("/tmp/test"));
        // Can't easily test anything that depends on qemu binaries, so we invoke
        // the internal builder to skip creating the real disk file.
        builder.build_internal().expect("Failed to build QCow2Disk")
    }

    #[test]
    fn test_qcow2disk() {
        let mut disk = build_test_qcow2disk(3);
        assert_eq!(
            disk.disk_file_name(),
            PathBuf::from("/tmp/test/test-device3.qcow2")
        );
        assert_eq!(disk.serial(), "test-device3");
        assert_eq!(
            &disk.qemu_args().join(OsStr::new(" ")),
            "-blockdev \
            driver=qcow2,node-name=test-device3,file.driver=file,file.filename=/tmp/test/test-device3.qcow2 \
            -device virtio-blk,bus=pci0,drive=test-device3,serial=test-device3,\
            physical_block_size=512,logical_block_size=512"
        );

        // Test serial override
        disk.opts.serial = Some("serial".to_string());
        assert_eq!(disk.serial(), "serial");
        assert_eq!(
            &disk.qemu_args().join(OsStr::new(" ")),
            "-blockdev \
            driver=qcow2,node-name=test-device3,file.driver=file,file.filename=/tmp/test/test-device3.qcow2 \
            -device virtio-blk,bus=pci0,drive=test-device3,serial=serial,\
            physical_block_size=512,logical_block_size=512"
        );

        // Test SATA drive
        disk.opts.interface = "ide-hd".into();
        assert_eq!(
            &disk.qemu_args().join(OsStr::new(" ")),
            "-blockdev \
            driver=qcow2,node-name=test-device3,file.driver=file,file.filename=/tmp/test/test-device3.qcow2 \
            -device ahci,id=ahci-test-device3,bus=pci0 \
            -device ide-hd,bus=ahci-test-device3.0,drive=test-device3,serial=serial,\
            physical_block_size=512,logical_block_size=512"
        );
    }

    #[test]
    fn test_qcow2disks() {
        let disk1 = build_test_qcow2disk(0);
        let mut disk2 = build_test_qcow2disk(1);
        disk2.opts.interface = "nvme".into();
        let disks = QCow2Disks(vec![disk1, disk2]);
        assert_eq!(
            &disks.qemu_args().join(OsStr::new(" ")),
            "-blockdev \
            driver=qcow2,node-name=test-device0,file.driver=file,file.filename=/tmp/test/test-device0.qcow2 \
            -device virtio-blk,bus=pci0,drive=test-device0,serial=test-device0,\
            physical_block_size=512,logical_block_size=512 \
            -blockdev \
            driver=qcow2,node-name=test-device1,file.driver=file,file.filename=/tmp/test/test-device1.qcow2 \
            -device nvme,bus=pci0,drive=test-device1,serial=test-device1,\
            physical_block_size=512,logical_block_size=512"
        );
    }
}
