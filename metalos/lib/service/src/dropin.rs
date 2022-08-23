/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use maplit::btreemap;
use metalos_host_configs::runtime_config::Service;
use serde::ser::Error as _;
use serde::ser::SerializeSeq;
use serde::ser::Serializer;
use serde::Deserialize;
use serde::Serialize;
use state::Alias;
use state::State;
use systemd::UnitName;
use uuid::Uuid;

use crate::unit_file::Environment;
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
    pub(crate) svc: Service,
    pub(crate) run_uuid: Uuid,
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
    alias: Alias<ServiceInstance>,
    unit: UnitSection,
    service: ServiceSection,
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
        let alias = token
            .alias(Alias::custom(svc.name().to_owned()))
            .context("while writing alias for ServiceInstance")?;
        let manager_unit: UnitName = format!(
            "metalos-native-service@{}.service",
            systemd::escape(svc.name())
        )
        .into();
        Ok(Dropin {
            alias,
            unit: UnitSection {
                after: manager_unit.clone(),
                requires: manager_unit.clone(),
                propagates_stop_to: manager_unit,
                description: UnitMetadata {
                    svc: svc.svc.clone(),
                    run_uuid: svc.run_uuid(),
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
    use anyhow::Result;
    use metalos_host_configs::runtime_config::ServiceType;
    use metalos_macros::containertest;

    use super::*;

    #[containertest]
    async fn dropin() -> Result<()> {
        let svc = ServiceInstance::new(Service {
            svc: metalos_host_configs::packages::Service::new(
                "metalos.service.demo".into(),
                "00000000000040008000000000000001"
                    .parse()
                    .expect("valid uuid"),
                None,
            ),
            config_generator: None,
            svc_type: Some(ServiceType::NATIVE),
        });
        let di = Dropin::new(&svc)?;
        // this is partially a sanity check on the serde_systemd crate, but is
        // also useful to see that the dropin is doing what we expect
        // NOTE: the extra blank lines found here are not important but is an
        // artifact of serde_systemd's output. If extra linebreaks are removed
        // from serde_systemd they should also be removed from here
        pretty_assertions::assert_eq!(
            format!(
                "[Unit]\n\
                 After=metalos-native-service@{alias}.service\n\
                 Requires=metalos-native-service@{alias}.service\n\
                 PropagatesStopTo=metalos-native-service@{alias}.service\n\
                 Description={{\"svc\":{{\"svc\":{{\"format\":1,\"id\":{{\"uuid\":\"00000000000040008000000000000001\"}},\"kind\":5,\"name\":\"metalos.service.demo\"}},\"svc_type\":0}},\"run_uuid\":\"{uuid}\"}}\n\
                 [Service]\n\
                 RootDirectory=/run/fs/control/run/service-roots/metalos.service.demo-00000000000040008000000000000001-{uuid_simple}\n\
                 Environment=CACHE_DIRECTORY=/metalos/cache\n\
                 Environment=LOGS_DIRECTORY=/metalos/logs\n\
                 Environment=METALOS_RUN_ID={uuid_simple}\n\
                 Environment=METALOS_VERSION=00000000000040008000000000000001\n\
                 Environment=RUNTIME_DIRECTORY=/metalos/runtime\n\
                 Environment=STATE_DIRECTORY=/metalos/state\n\
                 \n\
                 BindPaths=/run/fs/control/run/cache/metalos.service.demo:/metalos/cache\n\
                 BindPaths=/run/fs/control/run/logs/metalos.service.demo:/metalos/logs\n\
                 BindPaths=/run/fs/control/run/runtime/metalos.service.demo-00000000000040008000000000000001-{uuid_simple}:/metalos/runtime\n\
                 BindPaths=/run/fs/control/run/state/metalos.service.demo:/metalos/state\n\
                 \n",
                alias = systemd::escape(di.alias.to_string()),
                uuid = svc.run_uuid,
                uuid_simple = svc.run_uuid.to_simple(),
            ),
            serde_systemd::to_string(&di)?
        );
        Ok(())
    }
}
