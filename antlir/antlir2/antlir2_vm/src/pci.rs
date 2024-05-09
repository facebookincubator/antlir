/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;

use thiserror::Error;

use crate::types::QemuDevice;

/// Up to 32 devices can be attached to a single PCI bridge
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
}

impl QemuDevice for PCIBridge {
    fn qemu_args(&self) -> Vec<OsString> {
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

#[derive(Debug)]
pub(crate) struct PCIBridges(Vec<PCIBridge>);

impl PCIBridges {
    pub(crate) fn new(num_devices: usize) -> Result<Self> {
        let num_bridges = (num_devices + DEVICE_PER_BRIDGE - 1) / DEVICE_PER_BRIDGE;
        let bridges: Result<Vec<_>> = (0..num_bridges)
            .map(|i| -> Result<PCIBridge> { PCIBridge::new(i, i + 1) })
            .collect();
        Ok(Self(bridges?))
    }

    /// Get the PCI bridge for a given device ID
    pub(crate) fn bridge_for_device_id(&self, id: usize) -> &PCIBridge {
        let bridge_id = id / DEVICE_PER_BRIDGE;
        &self.0[bridge_id]
    }
}

impl QemuDevice for PCIBridges {
    fn qemu_args(&self) -> Vec<OsString> {
        self.0.iter().flat_map(|x| x.qemu_args()).collect()
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

    #[test]
    fn test_pcibridges() {
        let bridges = PCIBridges::new(10).expect("failed to create PCI bridge");
        assert_eq!(
            &bridges.qemu_args().join(OsStr::new(" ")),
            "-device pci-bridge,id=pci0,chassis_nr=1,shpc=off",
        );

        let bridges = PCIBridges::new(40).expect("failed to create PCI bridge");
        assert_eq!(
            &bridges.qemu_args().join(OsStr::new(" ")),
            "-device pci-bridge,id=pci0,chassis_nr=1,shpc=off -device pci-bridge,id=pci1,chassis_nr=2,shpc=off",
        );
    }

    #[test]
    fn test_bridge_for_device_id() {
        let bridges = PCIBridges::new(63).expect("failed to create PCI bridge");
        assert_eq!(bridges.bridge_for_device_id(0).name(), bridges.0[0].name());
        assert_eq!(bridges.bridge_for_device_id(4).name(), bridges.0[0].name());
        assert_eq!(bridges.bridge_for_device_id(32).name(), bridges.0[1].name());
        assert_eq!(bridges.bridge_for_device_id(36).name(), bridges.0[1].name());
    }
}
