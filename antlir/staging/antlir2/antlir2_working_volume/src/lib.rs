/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct WorkingVolume(PathBuf);

impl WorkingVolume {
    /// Ensure this [WorkingVolume] exists and is set up correctly.
    pub fn ensure(path: PathBuf) -> Result<Self> {
        if path.exists() {
            Ok(Self(path))
        } else {
            // If we're on Eden, create a new redirection
            // https://www.internalfb.com/intern/wiki/EdenFS/detecting-an-eden-mount/#on-linux-and-macos
            if Path::new(".eden").exists() {
                let res = Command::new("eden")
                    .arg("redirect")
                    .arg("add")
                    .arg(&path)
                    .arg("bind")
                    .spawn()?
                    .wait()?;
                if res.success() {
                    Ok(Self(path))
                } else {
                    Err(Error::new(
                        ErrorKind::Other,
                        format!("'eden redirect add' failed: {res}"),
                    ))
                }
            } else if let Err(e) = std::fs::create_dir(&path) {
                match e.kind() {
                    ErrorKind::AlreadyExists => Ok(Self(path)),
                    _ => Err(e),
                }
            } else {
                Ok(Self(path))
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Deref for WorkingVolume {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for WorkingVolume {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}
