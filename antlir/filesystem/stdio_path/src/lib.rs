/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::os::unix::io::FromRawFd;
use std::path::Path;

/// Attempts to open a file in read-only mode.
/// If path is '-', stdin will be opened.
/// Uses [File::open]
pub fn open<P: AsRef<Path>>(path: P) -> std::io::Result<File> {
    if path.as_ref() == Path::new("-") {
        let stdin_fd = std::io::stdin().as_raw_fd();
        Ok(unsafe { File::from_raw_fd(stdin_fd) })
    } else {
        File::open(path)
    }
}

/// Opens a file in write-only mode.
/// If path is '-', stdout will be opened.
/// Uses [File::create]
pub fn create<P: AsRef<Path>>(path: P) -> std::io::Result<File> {
    if path.as_ref() == Path::new("-") {
        let stdout_fd = std::io::stdout().as_raw_fd();
        Ok(unsafe { File::from_raw_fd(stdout_fd) })
    } else {
        File::create(path)
    }
}
