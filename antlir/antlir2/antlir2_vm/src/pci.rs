/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;

use thiserror::Error;

pub(crate) const DEVICE_PER_BRIDGE: usize = 32;

/// PCI Bridge. Each bridge can attach 32 devices
#[derive(Debug, Clone)]
pub(crate) struct PCIBridge {
    /// The ID of the bridge, starting from 0
    id: usize,
    /// Chassis ID
    chassis_id: u8,
}

#[derive(Error, Debug)]
pub(crate) enum PCIBridgeError {
    #[error("Chassis ID must fit into a u8. Got: {0}")]
    ChassisIDExceededError(usize),
}
type Result<T> = std::result::Result<T, PCIBridgeError>;

impl PCIBridge {
    pub(crate) fn new(id: usize, chassis_id: usize) -> Result<Self> {
        let chassis_id = chassis_id
            .try_into()
            .map_err(|_| PCIBridgeError::ChassisIDExceededError(chassis_id))?;
        Ok(Self { id, chassis_id })
    }

    /// Name of the bridge other devices can use to attach to
    pub(crate) fn name(&self) -> String {
        format!("pci{}", self.id)
    }

    /// Qemu arguments to create the bridge
    pub(crate) fn qemu_args(&self) -> Vec<OsString> {
        vec![
            "-device".into(),
            format!(
                "pci-bridge,id={},chassis_nr={},shpc=off",
                self.name(),
                self.chassis_id
            )
            .into(),
        ]
    }
}

#[cfg(test)]
mod test {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn test_pcibridge() {
        assert!(PCIBridge::new(1, 10000000).is_err());

        let bridge = PCIBridge::new(0, 1).expect("failed to create PCI bridge");
        assert_eq!(bridge.name(), "pci0");
        assert_eq!(
            &bridge.qemu_args().join(OsStr::new(" ")),
            "-device pci-bridge,id=pci0,chassis_nr=1,shpc=off",
        )
    }
}
