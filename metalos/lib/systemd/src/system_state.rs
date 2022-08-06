/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Ordering;

use systemd_macros::SystemdEnum;

use crate::Error;
use crate::Result;

/// The running state of the system as reported by
/// [ManagerProxy::system_state](crate::systemd_manager::ManagerProxy::system_state).
#[derive(Debug, PartialEq, Eq, Copy, Clone, SystemdEnum)]
pub enum SystemState {
    // Early bootup, before basic.target is reached or the maintenance state
    // entered.
    Initializing,
    // Late bootup, before the job queue becomes idle for the first time, or one
    // of the rescue targets are reached.
    Starting,
    // The system is fully operational.
    Running,
    // The system is operational but one or more units failed.
    Degraded,
    // The rescue or emergency target is active.
    Maintenance,
    // The manager is shutting down.
    Stopping,
    // The manager is not running. Specifically, this is the operational state
    // if an incompatible program is running as system manager (PID 1).
    Offline,
    // The operational state could not be determined, due to lack of resources
    // or another error cause.
    // Unknown,
}

/// A subset of SystemStates that are desirable for waiting on with
/// [Systemd::wait](crate::Systemd::wait).
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum WaitableSystemState {
    Initializing,
    Starting,
    Running,
    Degraded,
    // Virtual state representing the union of Running and Degraded.
    Operational,
    Maintenance,
    Stopping,
}

impl TryFrom<SystemState> for WaitableSystemState {
    type Error = Error;

    fn try_from(s: SystemState) -> Result<Self> {
        match s {
            SystemState::Initializing => Ok(Self::Initializing),
            SystemState::Starting => Ok(Self::Starting),
            SystemState::Running => Ok(Self::Running),
            SystemState::Degraded => Ok(Self::Degraded),
            SystemState::Maintenance => Ok(Self::Maintenance),
            SystemState::Stopping => Ok(Self::Stopping),
            _ => Err(crate::Error::SystemState(
                s,
                "cannot convert to WaitableSystemState",
            )),
        }
    }
}

impl Ord for WaitableSystemState {
    fn cmp(&self, other: &Self) -> Ordering {
        // use a match so that we can guarantee that the ordering implementation
        // is exhaustive
        match (self, other) {
            (Self::Initializing, Self::Initializing) => Ordering::Equal,
            (Self::Initializing, _) => Ordering::Less,

            (Self::Starting, Self::Initializing) => Ordering::Greater,
            (Self::Starting, Self::Starting) => Ordering::Equal,
            (Self::Starting, _) => Ordering::Less,

            (Self::Running, Self::Initializing) => Ordering::Greater,
            (Self::Running, Self::Starting) => Ordering::Greater,
            (Self::Running, Self::Running) => Ordering::Equal,
            (Self::Running, Self::Degraded) => Ordering::Equal,
            (Self::Running, Self::Operational) => Ordering::Equal,
            (Self::Running, Self::Maintenance) => Ordering::Equal,

            (Self::Degraded, Self::Initializing) => Ordering::Greater,
            (Self::Degraded, Self::Starting) => Ordering::Greater,
            (Self::Degraded, Self::Running) => Ordering::Equal,
            (Self::Degraded, Self::Degraded) => Ordering::Equal,
            (Self::Degraded, Self::Operational) => Ordering::Equal,
            (Self::Degraded, Self::Maintenance) => Ordering::Equal,

            (Self::Operational, Self::Initializing) => Ordering::Greater,
            (Self::Operational, Self::Starting) => Ordering::Greater,
            (Self::Operational, Self::Running) => Ordering::Equal,
            (Self::Operational, Self::Degraded) => Ordering::Equal,
            (Self::Operational, Self::Operational) => Ordering::Equal,
            (Self::Operational, Self::Maintenance) => Ordering::Equal,

            (Self::Maintenance, Self::Initializing) => Ordering::Greater,
            (Self::Maintenance, Self::Starting) => Ordering::Greater,
            (Self::Maintenance, Self::Running) => Ordering::Equal,
            (Self::Maintenance, Self::Degraded) => Ordering::Equal,
            (Self::Maintenance, Self::Operational) => Ordering::Equal,
            (Self::Maintenance, Self::Maintenance) => Ordering::Equal,

            (Self::Stopping, Self::Stopping) => Ordering::Equal,
            (Self::Stopping, _) => Ordering::Greater,
            (_, Self::Stopping) => Ordering::Less,
        }
    }
}

impl PartialOrd for WaitableSystemState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use itertools::Itertools;

    use super::WaitableSystemState;

    #[test]
    fn system_state_ord() {
        assert!(WaitableSystemState::Initializing < WaitableSystemState::Starting);
        assert!(WaitableSystemState::Initializing < WaitableSystemState::Running);
        assert!(WaitableSystemState::Initializing < WaitableSystemState::Degraded);
        assert!(WaitableSystemState::Initializing < WaitableSystemState::Stopping);
        assert!(WaitableSystemState::Initializing < WaitableSystemState::Maintenance);
        assert!(WaitableSystemState::Starting < WaitableSystemState::Running);
        assert!(WaitableSystemState::Starting < WaitableSystemState::Degraded);
        assert!(WaitableSystemState::Starting < WaitableSystemState::Stopping);
        assert!(WaitableSystemState::Starting < WaitableSystemState::Maintenance);
        // now the ordering gets a little more complicated, all these states are
        // considered equal in the boot timeline
        let operational_states = [
            WaitableSystemState::Operational,
            WaitableSystemState::Running,
            WaitableSystemState::Degraded,
            WaitableSystemState::Maintenance,
        ];
        for (a, b) in operational_states
            .iter()
            .cartesian_product(operational_states)
        {
            assert_eq!(a.partial_cmp(&b), Some(Ordering::Equal));
        }
        for s in operational_states {
            assert!(
                s < WaitableSystemState::Stopping,
                "{:?} should be before Stopping",
                s
            );
        }
    }
}
