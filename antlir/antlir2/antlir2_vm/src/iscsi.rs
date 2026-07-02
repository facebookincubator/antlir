/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::process::Child;
use std::process::Command;
use std::thread;
use std::time::Duration;

use thiserror::Error;
use tracing::debug;

use crate::utils::log_command;
use crate::utils::run_command_capture_output;

pub(crate) const TGTD_PORTAL: &str = "[::]:3260";
pub(crate) const ISCSI_LUN: u32 = 1;

pub(crate) fn target_iqn(disk_id: usize) -> String {
    format!("iqn.2024-01.com.meta.vmtest:disk{disk_id}")
}

#[derive(Debug, Error)]
pub(crate) enum IscsiError {
    #[error("Failed to start tgtd: {0}")]
    TgtdStartError(std::io::Error),
    #[error("tgtd did not become ready within 5 seconds")]
    TgtdReadyTimeout,
    #[error("tgtadm failed: {0}")]
    TgtadmError(std::io::Error),
}

type Result<T> = std::result::Result<T, IscsiError>;

pub(crate) struct IscsiTargetDaemon {
    process: Child,
}

impl std::fmt::Debug for IscsiTargetDaemon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IscsiTargetDaemon")
            .field("pid", &self.process.id())
            .finish()
    }
}

impl Drop for IscsiTargetDaemon {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

impl IscsiTargetDaemon {
    pub(crate) fn start(state_dir: &Path) -> Result<Self> {
        let process = Self::start_tgtd(state_dir)?;
        Self::wait_for_ready()?;
        Ok(Self { process })
    }

    fn start_tgtd(state_dir: &Path) -> Result<Child> {
        let pid_file = state_dir.join("tgtd.pid");
        let mut cmd = Command::new("tgtd");
        cmd.arg("--foreground")
            .arg("--iscsi")
            .arg(format!("portal={TGTD_PORTAL}"))
            .arg("--pid-file")
            .arg(&pid_file);
        log_command(&mut cmd)
            .spawn()
            .map_err(IscsiError::TgtdStartError)
    }

    fn wait_for_ready() -> Result<()> {
        for _ in 0..50 {
            let result = Command::new("tgtadm")
                .args(["--lld", "iscsi", "--mode", "system", "--op", "show"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if let Ok(status) = result {
                if status.success() {
                    debug!("tgtd is ready");
                    return Ok(());
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(IscsiError::TgtdReadyTimeout)
    }

    pub(crate) fn add_target(&mut self, disk_id: usize, backing_file: &Path) -> Result<()> {
        let tid = (disk_id + 1) as u32;
        let iqn = target_iqn(disk_id);

        run_command_capture_output(
            Command::new("tgtadm")
                .args(["--lld", "iscsi", "--mode", "target", "--op", "new"])
                .arg("-t")
                .arg(tid.to_string())
                .arg("-T")
                .arg(&iqn),
        )
        .map_err(IscsiError::TgtadmError)?;

        run_command_capture_output(
            Command::new("tgtadm")
                .args(["--lld", "iscsi", "--mode", "logicalunit", "--op", "new"])
                .arg("-t")
                .arg(tid.to_string())
                .arg("--lun")
                .arg(ISCSI_LUN.to_string())
                .arg("--backing-store")
                .arg(backing_file),
        )
        .map_err(IscsiError::TgtadmError)?;

        run_command_capture_output(
            Command::new("tgtadm")
                .args(["--lld", "iscsi", "--mode", "target", "--op", "bind"])
                .arg("-t")
                .arg(tid.to_string())
                .arg("-I")
                .arg("ALL"),
        )
        .map_err(IscsiError::TgtadmError)?;

        debug!(
            tid,
            iqn,
            backing_file = %backing_file.display(),
            "Created iSCSI target"
        );

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_target_iqn() {
        assert_eq!(target_iqn(0), "iqn.2024-01.com.meta.vmtest:disk0");
        assert_eq!(target_iqn(3), "iqn.2024-01.com.meta.vmtest:disk3");
    }
}
