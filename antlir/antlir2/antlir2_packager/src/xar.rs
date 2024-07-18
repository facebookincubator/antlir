/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::fs::Permissions;
use std::io::BufWriter;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use serde::Deserialize;
use uuid::Uuid;

use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Xar {
    squashfs: PathBuf,
    executable: PathBuf,
    target_name: String,
}

const SQUASHFS_OFFSET: u64 = 4096;

impl PackageFormat for Xar {
    fn build(&self, out: &Path) -> Result<()> {
        let mut out = BufWriter::new(File::create(out)?);
        writeln!(out, "#!/usr/bin/env xarexec_fuse")?;
        writeln!(
            out,
            "OFFSET=\"{SQUASHFS_OFFSET}\"\n\
            UUID=\"{uuid}\"\n\
            VERSION=\"{timestamp}\"\n\
            DEPENDENCIES=\"\"\n\
            XAREXEC_TARGET=\"{executable}\"\n\
            XAREXEC_TRAMPOLINE_NAMES=\"'{target_name}' 'invoke_xar_via_trampoline'\"\n\
            #xar_stop\n\
            echo This XAR file should not be executed by sh\n\
            exit 1\n\
            # Actual squashfs file begins at {SQUASHFS_OFFSET}",
            executable = self.executable.display(),
            uuid = &Uuid::new_v4().to_string()[..8],
            timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
            target_name = self.target_name,
        )?;
        out.seek(SeekFrom::Start(SQUASHFS_OFFSET))?;
        let mut squashfs = File::open(&self.squashfs)?;
        std::io::copy(&mut squashfs, &mut out)?;
        out.into_inner()?
            .set_permissions(Permissions::from_mode(0o755))?;
        Ok(())
    }
}
