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
use systemd::ActiveState;
use systemd::Systemd;

use crate::dropin::UnitMetadata;
use crate::Version;

/// Describe a full set of native services. This may be either the versions that
/// are currently running, or desired versions.
#[derive(Debug)]
pub struct ServiceSet(BTreeMap<String, Version>);

impl ServiceSet {
    /// Load the set of MetalOS native services that are currently running.
    pub async fn current(sd: &Systemd) -> Result<Self> {
        let units = sd.list_units().await.context("while listing all units")?;
        let mut native_services = BTreeMap::new();
        for u in units {
            match u.active_state {
                ActiveState::Active | ActiveState::Reloading | ActiveState::Activating => {}
                _ => continue,
            }
            // if this parses to this json struct, then it's a metalos service,
            // otherwise just ignore it
            match serde_json::from_str::<UnitMetadata>(&u.description) {
                Ok(meta) => {
                    native_services.insert(meta.native_service, meta.version);
                }
                Err(_) => {}
            };
        }
        Ok(Self::new(native_services))
    }

    pub fn new(map: BTreeMap<String, Version>) -> Self {
        Self(map)
    }

    /// Compute a set of service modifications that must be done in order to
    /// make the `self` set match the desired `other` set.
    pub(crate) fn diff(&self, other: &Self) -> Diff {
        let mut diff: BTreeMap<String, _> = BTreeMap::new();
        let self_keys: BTreeSet<_> = self.0.keys().collect();
        let other_keys: BTreeSet<_> = other.0.keys().collect();
        for removed in self_keys.difference(&other_keys) {
            diff.insert((*removed).clone(), ServiceDiff::Stop(self.0[*removed]));
        }
        for added in other_keys.difference(&self_keys) {
            diff.insert((*added).clone(), ServiceDiff::Start(other.0[*added]));
        }
        for maybe_changed in self_keys.intersection(&other_keys) {
            let old = self.0[*maybe_changed];
            let new = other.0[*maybe_changed];
            if old != new {
                diff.insert(
                    (*maybe_changed).clone(),
                    ServiceDiff::Swap {
                        current: old,
                        next: new,
                    },
                );
            }
        }

        Diff(diff)
    }
}

impl Deref for ServiceSet {
    type Target = BTreeMap<String, Version>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ServiceSet {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ServiceDiff {
    /// Start this version when no version of the service is currently running
    Start(Version),
    /// Swap from the current version to the next
    Swap { current: Version, next: Version },
    /// Stop this service
    Stop(Version),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Diff(pub(crate) BTreeMap<String, ServiceDiff>);

impl Deref for Diff {
    type Target = BTreeMap<String, ServiceDiff>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use maplit::btreemap;
    use metalos_macros::test;
    use pretty_assertions::assert_eq;

    use super::*;

    impl From<(Option<Version>, Option<Version>)> for ServiceDiff {
        fn from(pair: (Option<Version>, Option<Version>)) -> ServiceDiff {
            match pair {
                (Some(current), Some(next)) => ServiceDiff::Swap { current, next },
                (Some(current), None) => ServiceDiff::Stop(current),
                (None, Some(next)) => ServiceDiff::Start(next),
                (None, None) => panic!("None => None is invalid"),
            }
        }
    }

    macro_rules! service_set {
        ($($n:literal => $v:literal,)*) => (
            crate::set::ServiceSet::new(maplit::btreemap! {
                $($n.into() => format!("00000000-0000-4000-8000-000000000{:03}", $v).parse().unwrap()),*
            })
        );
        ($n:literal => $v:literal) => (service_set! {$n => $v,});
        () => (crate::set::ServiceSet::new(std::collections::BTreeMap::new()));
    }

    pub(crate) use service_set;

    macro_rules! diff {
        ($($n:literal: $old:expr => $new:expr,)*) => (
            Diff(btreemap! {
                $($n.into() => ServiceDiff::from((
                    $old.map(|x: u8| format!("00000000-0000-4000-8000-000000000{:03}", x).parse().unwrap()),
                    $new.map(|x: u8| format!("00000000-0000-4000-8000-000000000{:03}", x).parse().unwrap()),
                ))),*
            })
        );
        ($n:literal: $old:expr => $new:expr) => (diff! {$n: $old => $new,});
    }

    #[test]
    fn diff() {
        assert_eq!(
            diff! { "a": None => Some(1) },
            service_set!().diff(&service_set! {"a" => 1})
        );
        assert_eq!(
            diff! { "a": Some(2) => None },
            service_set! {"a" => 2}.diff(&service_set! {})
        );
        assert_eq!(
            diff! { "a": Some(2) => Some(3) },
            service_set! {"a" => 2}.diff(&service_set! {"a" => 3})
        );
        assert_eq!(
            diff! { "a": Some(2) => Some(3), "b": Some(3) => None, "c": None => Some(2), },
            service_set! {"a" => 2, "b" => 3, }.diff(&service_set! {"a" => 3, "c" => 2, })
        );
    }
}
