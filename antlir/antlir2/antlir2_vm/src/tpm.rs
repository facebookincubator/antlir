/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use thiserror::Error;
use tracing::Level;
use tracing_subscriber::filter::LevelFilter;

use crate::runtime::get_runtime;

/// TPM 2.0 device
#[derive(Debug)]
pub(crate) struct TPMDevice {
    /// State directory for swtpm
    state_dir: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum TPMError {
    #[error("Failed to create state directory for TPM: {0}")]
    StateDirectoryError(std::io::Error),
    #[error("{msg}: {err}")]
    TPMProcessError { msg: String, err: std::io::Error },
}

type Result<T> = std::result::Result<T, TPMError>;

impl TPMDevice {
    /// Create a new TPM device and start the process
    pub(crate) fn new(parent_state_dir: &Path) -> Result<Self> {
        let state_dir = parent_state_dir.join("tpm");
        fs::create_dir(&state_dir).map_err(TPMError::StateDirectoryError)?;
        Self::start_tpm(state_dir.as_path())?;
        Ok(Self { state_dir })
    }

    pub(crate) fn qemu_args(&self) -> Vec<OsString> {
        [
            "-chardev",
            &format!(
                "socket,id=chrtpm,path={}",
                self.socket_path()
                    .to_str()
                    .expect("Invalid socket file path")
            ),
            "-tpmdev",
            "emulator,id=tpm0,chardev=chrtpm",
            "-device",
            "tpm-tis,tpmdev=tpm0",
        ]
        .iter()
        .map(|x| x.into())
        .collect()
    }

    fn socket_path(&self) -> PathBuf {
        self.state_dir.join("swtpm.sock")
    }

    fn start_tpm(state_dir: &Path) -> Result<()> {
        let mut command = Command::new(&get_runtime().swtpm);
        command
            .arg("socket")
            .arg("--tpm2")
            .arg("--tpmstate")
            .arg(format!(
                "dir={}",
                state_dir.to_str().expect("Invalid directory for tpm state")
            ))
            .arg("--ctrl")
            .arg(format!(
                "type=unixio,path={}",
                state_dir
                    .join("swtpm.sock")
                    .to_str()
                    .expect("Invalid socket file path")
            ));
        if LevelFilter::current() >= Level::from_str("debug").expect("Invalid logging level") {
            command.arg("--log").arg("level=20");
        }
        command.spawn().map_err(|err| TPMError::TPMProcessError {
            msg: format!("Failed to spawn {}", get_runtime().swtpm),
            err,
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn test_tpm() {
        let tmp_dir = tempfile::tempdir().expect("Failed to create temporary directory");
        let tpm = TPMDevice {
            state_dir: tmp_dir.path().to_owned(),
        };
        assert_eq!(
            tpm.qemu_args().join(OsStr::new(" ")),
            OsStr::new(&format!(
                "-chardev socket,id=chrtpm,path={} \
                    -tpmdev emulator,id=tpm0,chardev=chrtpm \
                    -device tpm-tis,tpmdev=tpm0",
                tmp_dir
                    .path()
                    .join("swtpm.sock")
                    .to_str()
                    .expect("Invalid socket file path"),
            )),
        );
    }
}
