/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Run details for a single execution of a Native Service.
#[derive(Debug, Deserialize, Serialize)]
pub struct ServiceInstance {
    name: String,
    version: Uuid,
    run_uuid: Uuid,
    paths: Paths,
}

impl ServiceInstance {
    pub fn new(name: String, version: Uuid) -> Self {
        let run_uuid = Uuid::new_v4();
        let unique = format!("{}-{}-{}", name, version.to_simple(), run_uuid.to_simple());
        let base = Path::new("/run/fs/control/run");
        let paths = Paths {
            root_source: Path::new("/run/fs/control/image/service").join(format!(
                "{}:{}/volume",
                name,
                version.to_simple()
            )),
            root: base.join("service_roots").join(&unique),
            state: base.join("state").join(&name),
            cache: base.join("cache").join(&name),
            logs: base.join("logs").join(&name),
            runtime: base.join("runtime").join(unique),
        };
        Self {
            name,
            version,
            run_uuid,
            paths,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> Uuid {
        self.version
    }

    pub fn run_uuid(&self) -> Uuid {
        self.run_uuid
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }
}

#[derive(Debug, Deserialize, Serialize)]
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
