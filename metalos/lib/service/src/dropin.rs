/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use maplit::btreemap;
use serde::ser::Error as _;
use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use state::{State, Token};
use systemd::UnitName;

use crate::ServiceInstance;

const MOUNT_CACHE: &str = "/metalos/cache";
const MOUNT_LOGS: &str = "/metalos/logs";
const MOUNT_RUNTIME: &str = "/metalos/runtime";
const MOUNT_STATE: &str = "/metalos/state";

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct UnitSection {
    after: UnitName,
    requires: UnitName,
    propagates_stop_to: UnitName,
    /// Description is available when listing all loaded units, so use it to
    /// store some very high-level metadata to enable discovery of all metalos
    /// native services
    #[serde(with = "serde_with::json::nested")]
    description: UnitMetadata,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct UnitMetadata {
    pub(crate) native_service: String,
    pub(crate) version: Uuid,
}

// At some point it would be nice to bootcamp moving this into `serde_systemd`
// or a companion crate, but I(vmagro) want to wait a little bit on that until I
// can collect a more useful set of primitives and specialized types (based
// primarily on usage that will spring up in this crate)
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Environment(pub(crate) BTreeMap<String, String>);

impl Serialize for Environment {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = ser.serialize_seq(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            seq.serialize_element(&format!("{}={}", k, v))?;
        }
        seq.end()
    }
}

fn serialize_bind_rw_paths<S>(paths: &BTreeMap<PathBuf, PathBuf>, ser: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut seq = ser.serialize_seq(Some(paths.len()))?;
    for (src, dst) in paths {
        match (src.to_str(), dst.to_str()) {
            (Some(src), Some(dst)) => seq.serialize_element(&format!("{}:{}", src, dst)),
            (None, Some(_)) => Err(S::Error::custom(format!("src ({:?}) not utf-8", src))),
            (Some(_), None) => Err(S::Error::custom(format!("dst ({:?}) not utf-8", dst))),
            (None, None) => Err(S::Error::custom(format!(
                "neither src ({:?}) nor dst ({:?}) are utf-8",
                src, dst
            ))),
        }?;
    }
    seq.end()
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct ServiceSection {
    root_directory: PathBuf,
    environment: Environment,
    #[serde(rename = "BindPaths", serialize_with = "serialize_bind_rw_paths")]
    bind_rw_paths: BTreeMap<PathBuf, PathBuf>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Dropin {
    #[serde(skip)]
    token: Token<ServiceInstance>,
    unit: UnitSection,
    service: ServiceSection,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ManagerDropin {
    unit: ManagerUnitSection,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ManagerUnitSection {
    part_of: UnitName,
}

impl Dropin {
    pub(crate) fn new(svc: &ServiceInstance) -> Result<Self> {
        let paths = svc.paths();
        let bind_rw_paths = btreemap! {
            paths.state().to_owned() => MOUNT_STATE.into(),
            paths.cache().to_owned() => MOUNT_CACHE.into(),
            paths.logs().to_owned() => MOUNT_LOGS.into(),
            paths.runtime().to_owned() => MOUNT_RUNTIME.into(),
        };
        let token = svc
            .save()
            .context("while saving ServiceInstance to the state store")?;
        let manager_unit: UnitName = format!(
            "metalos-native-service@{}.service",
            systemd::escape(token.to_string())
        )
        .into();
        // annoyingly, we also have to generate a dropin for the native service
        // manager so that it will be restarted when the native service is
        // restarted
        let manager_dropin = ManagerDropin {
            unit: ManagerUnitSection {
                part_of: svc.unit_name().clone(),
            },
        };
        let manager_dropin_dir: PathBuf = format!("/run/systemd/system/{}.d", manager_unit).into();
        std::fs::create_dir_all(&manager_dropin_dir)
            .context("while creating manager dropin dir")?;
        let f = std::fs::File::create(manager_dropin_dir.join("99-native-service.conf"))
            .context("while creating manager dropin file")?;
        serde_systemd::to_writer(f, &manager_dropin).context("while writing manager dropin")?;
        Ok(Dropin {
            token,
            unit: UnitSection {
                after: manager_unit.clone(),
                requires: manager_unit.clone(),
                propagates_stop_to: manager_unit,
                description: UnitMetadata {
                    native_service: svc.name().to_string(),
                    version: svc.version(),
                },
            },
            service: ServiceSection {
                root_directory: paths.root().to_owned(),
                environment: Environment(btreemap! {
                    "CACHE_DIRECTORY".into() => MOUNT_CACHE.into(),
                    "LOGS_DIRECTORY".into() => MOUNT_LOGS.into(),
                    "RUNTIME_DIRECTORY".into() => MOUNT_RUNTIME.into(),
                    "STATE_DIRECTORY".into() => MOUNT_STATE.into(),
                    "METALOS_RUN_ID".into() => svc.run_uuid().to_simple().to_string(),
                    "METALOS_VERSION".into() => svc.version().to_simple().to_string(),
                }),
                bind_rw_paths,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use metalos_macros::containertest;

    #[containertest]
    async fn dropin() -> Result<()> {
        crate::tests::wait_for_systemd().await.unwrap();
        let svc = ServiceInstance::new(
            "metalos.service.demo".into(),
            "00000000000040008000000000000001".parse().unwrap(),
        );
        let di = Dropin::new(&svc)?;
        // this is partially a sanity check on the serde_systemd crate, but is
        // also useful to see that the dropin is doing what we expect
        // NOTE: the extra blank lines found here are not important but is an
        // artifact of serde_systemd's output. If extra linebreaks are removed
        // from serde_systemd they should also be removed from here
        pretty_assertions::assert_eq!(
            format!(
                "[Unit]\n\
                 After=metalos-native-service@{token}.service\n\
                 Requires=metalos-native-service@{token}.service\n\
                 PropagatesStopTo=metalos-native-service@{token}.service\n\
                 Description={{\"native_service\":\"metalos.service.demo\",\"version\":\"00000000-0000-4000-8000-000000000001\"}}\n\
                 [Service]\n\
                 RootDirectory=/run/fs/control/run/service-roots/metalos.service.demo-00000000000040008000000000000001-{uuid}\n\
                 Environment=CACHE_DIRECTORY=/metalos/cache\n\
                 Environment=LOGS_DIRECTORY=/metalos/logs\n\
                 Environment=METALOS_RUN_ID={uuid}\n\
                 Environment=METALOS_VERSION=00000000000040008000000000000001\n\
                 Environment=RUNTIME_DIRECTORY=/metalos/runtime\n\
                 Environment=STATE_DIRECTORY=/metalos/state\n\
                 \n\
                 BindPaths=/run/fs/control/run/cache/metalos.service.demo:/metalos/cache\n\
                 BindPaths=/run/fs/control/run/logs/metalos.service.demo:/metalos/logs\n\
                 BindPaths=/run/fs/control/run/runtime/metalos.service.demo-00000000000040008000000000000001-{uuid}:/metalos/runtime\n\
                 BindPaths=/run/fs/control/run/state/metalos.service.demo:/metalos/state\n\
                 \n",
                token = systemd::escape(di.token.to_string()),
                uuid = svc.run_uuid.to_simple(),
            ),
            serde_systemd::to_string(&di)?
        );
        Ok(())
    }
}
