/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::ops::Deref;
use std::ops::DerefMut;

use anyhow::Context;
use anyhow::Result;
use metalos_host_configs::runtime_config::Service;
use systemd::ActiveState;
use systemd::Systemd;

use crate::dropin::UnitMetadata;

/// Describe a full set of native services. This may be either the versions that
/// are currently running, or desired versions.
#[derive(Debug)]
pub struct ServiceSet(BTreeSet<Service>);

impl ServiceSet {
    pub fn new(iter: impl IntoIterator<Item = Service>) -> Self {
        Self(iter.into_iter().collect())
    }

    /// Load the set of MetalOS native services that are currently running.
    pub async fn current(sd: &Systemd) -> Result<Self> {
        let units = sd.list_units().await.context("while listing all units")?;
        let mut native_services = BTreeSet::new();
        for u in units {
            match u.active_state {
                ActiveState::Active | ActiveState::Reloading | ActiveState::Activating => {}
                _ => continue,
            }
            // if this parses to this json struct, then it's a metalos service,
            // otherwise just ignore it
            match serde_json::from_str::<UnitMetadata>(&u.description) {
                Ok(meta) => {
                    native_services.insert(meta.svc);
                }
                Err(_) => {}
            };
        }
        Ok(Self(native_services))
    }

    /// Compute a set of service modifications that must be done in order to
    /// make the `self` set match the desired `other` set.
    pub(crate) fn diff(&self, other: &Self) -> Diff {
        let mut diff = BTreeSet::new();
        let self_map: BTreeMap<_, _> = self.0.iter().map(|s| (s.name(), s.clone())).collect();
        let other_map: BTreeMap<_, _> = other.0.iter().map(|s| (s.name(), s.clone())).collect();
        let self_keys: BTreeSet<_> = self.0.iter().map(|s| s.name()).collect();
        let other_keys: BTreeSet<_> = other.0.iter().map(|s| s.name()).collect();
        for removed in self_keys.difference(&other_keys) {
            diff.insert(ServiceDiff::Stop(
                self_map
                    .get(removed)
                    .expect("already verified this exists")
                    .clone(),
            ));
        }
        for added in other_keys.difference(&self_keys) {
            diff.insert(ServiceDiff::Start(
                other_map
                    .get(added)
                    .expect("already verified this exists")
                    .clone(),
            ));
        }
        for maybe_changed in self_keys.intersection(&other_keys) {
            let old = self_map
                .get(maybe_changed)
                .clone()
                .expect("already verified this exists");
            let new = other_map
                .get(maybe_changed)
                .clone()
                .expect("already verified this exists");
            if old != new {
                diff.insert(ServiceDiff::Swap {
                    current: old.clone(),
                    next: new.clone(),
                });
            }
        }

        Diff(diff)
    }
}

impl Deref for ServiceSet {
    type Target = BTreeSet<Service>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ServiceSet {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ServiceDiff {
    /// Start this version when no version of the service is currently running
    Start(Service),
    /// Swap from the current version to the next
    Swap { current: Service, next: Service },
    /// Stop this service
    Stop(Service),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Diff(pub(crate) BTreeSet<ServiceDiff>);

impl Deref for Diff {
    type Target = BTreeSet<ServiceDiff>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use maplit::btreeset;
    use metalos_host_configs::packages::Service as ServicePackage;
    use metalos_host_configs::runtime_config::ServiceType;
    use metalos_macros::test;
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn diff() {
        let svc_a = Service {
            svc: ServicePackage::new("a".into(), Uuid::new_v4(), None),
            config_generator: None,
            svc_type: Some(ServiceType::NATIVE),
        };
        assert_eq!(
            Diff(btreeset! {ServiceDiff::Start(svc_a.clone())}),
            ServiceSet::new(vec![]).diff(&ServiceSet::new(vec![svc_a.clone()]))
        );
        assert_eq!(
            Diff(btreeset! {ServiceDiff::Stop(svc_a.clone())}),
            ServiceSet::new(vec![svc_a.clone()]).diff(&ServiceSet::new(vec![]))
        );
        let mut svc_a_alternate_version = svc_a.clone();
        svc_a_alternate_version.svc.id = Uuid::new_v4();
        assert_eq!(
            Diff(btreeset! {ServiceDiff::Swap {
                current: svc_a.clone(),
                next: svc_a_alternate_version.clone(),
            }}),
            ServiceSet::new(vec![svc_a.clone()])
                .diff(&ServiceSet::new(vec![svc_a_alternate_version.clone()]))
        );

        let svc_b = Service {
            svc: ServicePackage::new("b".into(), Uuid::new_v4(), None),
            config_generator: None,
            svc_type: Some(ServiceType::NATIVE),
        };
        let svc_c = Service {
            svc: ServicePackage::new("c".into(), Uuid::new_v4(), None),
            config_generator: None,
            svc_type: Some(ServiceType::NATIVE),
        };
        assert_eq!(
            Diff(btreeset! {
                ServiceDiff::Swap {
                    current: svc_a.clone(),
                    next: svc_a_alternate_version.clone(),
                },
                ServiceDiff::Start(svc_c.clone()),
                ServiceDiff::Stop(svc_b.clone()),
            }),
            ServiceSet::new(vec![svc_a, svc_b])
                .diff(&ServiceSet::new(vec![svc_a_alternate_version, svc_c]))
        );
    }
}
