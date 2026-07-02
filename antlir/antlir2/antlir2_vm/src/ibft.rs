/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::fs;
use std::net::Ipv6Addr;
use std::path::Path;
use std::path::PathBuf;

use thiserror::Error;
use tracing::debug;

use crate::types::QemuDevice;

#[derive(Debug, Error)]
pub(crate) enum IbftError {
    #[error("Failed to write iBFT file: {0}")]
    WriteError(std::io::Error),
}

type Result<T> = std::result::Result<T, IbftError>;

pub(crate) struct IbftTarget {
    pub iqn: String,
    pub portal_ip: Ipv6Addr,
    pub portal_port: u16,
    pub lun: u64,
}

pub(crate) struct IbftNic {
    pub ip: Ipv6Addr,
    pub prefix: u8,
    pub gateway: Ipv6Addr,
    pub mac: [u8; 6],
}

#[derive(Debug)]
pub(crate) struct IbftTable {
    path: PathBuf,
}

// The iBFT "table header" is a standard 36-byte ACPI header followed by 12
// reserved bytes (`struct acpi_table_ibft` = `acpi_table_header` + `reserved[12]`
// in the kernel). All structures are located relative to the start of the table,
// and the kernel reads the Control structure at `header + sizeof(acpi_table_ibft)`
// (i.e. offset 48). Using 36 here places every structure 12 bytes too early, so
// the kernel reads a bogus Control header and `iscsi_ibft` fails with -ENODEV.
const ACPI_HEADER_LEN: usize = 48;
const CONTROL_LEN: usize = 18;
const INITIATOR_LEN: usize = 74;
const NIC_LEN: usize = 102;
const TARGET_LEN: usize = 54;

const CONTROL_OFFSET: usize = ACPI_HEADER_LEN;
const INITIATOR_OFFSET: usize = CONTROL_OFFSET + CONTROL_LEN;
const NIC_OFFSET: usize = INITIATOR_OFFSET + INITIATOR_LEN;
const TARGET_OFFSET: usize = NIC_OFFSET + NIC_LEN;
const HEAP_OFFSET: usize = TARGET_OFFSET + TARGET_LEN;

impl IbftTable {
    pub(crate) fn generate(
        state_dir: &Path,
        initiator_iqn: &str,
        target: &IbftTarget,
        nic: &IbftNic,
    ) -> Result<Self> {
        let table = build_ibft(initiator_iqn, target, nic);
        let path = state_dir.join("ibft.bin");
        fs::write(&path, &table).map_err(IbftError::WriteError)?;
        debug!(path = %path.display(), size = table.len(), "Generated iBFT ACPI table");
        Ok(Self { path })
    }
}

impl QemuDevice for IbftTable {
    fn qemu_args(&self) -> Vec<OsString> {
        vec![
            "-acpitable".into(),
            format!("file={}", self.path.display()).into(),
        ]
    }
}

fn write_u16_le(buf: &mut [u8], offset: usize, val: u16) {
    buf[offset..offset + 2].copy_from_slice(&val.to_le_bytes());
}

fn write_u32_le(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
}

fn write_ipv6(buf: &mut [u8], offset: usize, addr: Ipv6Addr) {
    buf[offset..offset + 16].copy_from_slice(&addr.octets());
}

fn build_ibft(initiator_iqn: &str, target: &IbftTarget, nic: &IbftNic) -> Vec<u8> {
    let initiator_name = initiator_iqn.as_bytes();
    let target_name = target.iqn.as_bytes();
    let total_len = HEAP_OFFSET + initiator_name.len() + target_name.len();

    let mut buf = vec![0u8; total_len];

    // --- ACPI Header (36 bytes) + 12 reserved bytes = 48-byte table header ---
    buf[0..4].copy_from_slice(b"iBFT");
    write_u32_le(&mut buf, 4, total_len as u32);
    buf[8] = 1; // revision
    // buf[9] = checksum (filled at the end)
    buf[10..16].copy_from_slice(b"META  ");
    buf[16..24].copy_from_slice(b"VMTEST  ");
    write_u32_le(&mut buf, 24, 1); // OEM revision
    buf[28..32].copy_from_slice(b"META");
    write_u32_le(&mut buf, 32, 1); // creator revision

    // --- Control Structure (18 bytes at offset 48) ---
    let c = CONTROL_OFFSET;
    buf[c] = 1; // structure ID: control
    buf[c + 1] = 1; // version
    write_u16_le(&mut buf, c + 2, CONTROL_LEN as u16);
    // index=0, flags=0
    // extensions=0
    write_u16_le(&mut buf, c + 8, INITIATOR_OFFSET as u16);
    write_u16_le(&mut buf, c + 10, NIC_OFFSET as u16);
    write_u16_le(&mut buf, c + 12, TARGET_OFFSET as u16);
    // NIC 1 and Target 1 offsets stay 0

    // --- Initiator Structure (74 bytes at offset 66) ---
    let initiator_name_offset = HEAP_OFFSET;
    let i = INITIATOR_OFFSET;
    buf[i] = 2; // structure ID: initiator
    buf[i + 1] = 1; // version
    write_u16_le(&mut buf, i + 2, INITIATOR_LEN as u16);
    // index=0
    buf[i + 5] = 0x03; // flags: VALID | FIRMWARE_BOOT_SELECTED
    // iSNS, SLP, primary/secondary RADIUS servers: all zeros (offsets 6..70)
    write_u16_le(&mut buf, i + 70, initiator_name.len() as u16);
    write_u16_le(&mut buf, i + 72, initiator_name_offset as u16);

    // --- NIC Structure (102 bytes at offset 140) ---
    let n = NIC_OFFSET;
    buf[n] = 3; // structure ID: NIC
    buf[n + 1] = 1; // version
    write_u16_le(&mut buf, n + 2, NIC_LEN as u16);
    // index=0
    buf[n + 5] = 0x03; // flags: VALID | FIRMWARE_BOOT_SELECTED
    write_ipv6(&mut buf, n + 6, nic.ip);
    buf[n + 22] = nic.prefix; // subnet mask prefix
    buf[n + 23] = 0; // origin: static
    write_ipv6(&mut buf, n + 24, nic.gateway);
    // DNS, DHCP: all zeros
    // VLAN=0
    buf[n + 90..n + 96].copy_from_slice(&nic.mac);
    // PCI bus/dev/func=0, hostname length/offset=0

    // --- Target Structure (54 bytes at offset 242) ---
    let target_name_offset = initiator_name_offset + initiator_name.len();
    let t = TARGET_OFFSET;
    buf[t] = 4; // structure ID: target
    buf[t + 1] = 1; // version
    write_u16_le(&mut buf, t + 2, TARGET_LEN as u16);
    // index=0
    buf[t + 5] = 0x03; // flags: VALID | FIRMWARE_BOOT_SELECTED
    write_ipv6(&mut buf, t + 6, target.portal_ip);
    write_u16_le(&mut buf, t + 22, target.portal_port);
    // Target Boot LUN (8 bytes, SCSI LUN encoding: LUN in second byte pair)
    buf[t + 25] = target.lun as u8;
    // CHAP type=0 (no auth), NIC association=0
    write_u16_le(&mut buf, t + 34, target_name.len() as u16);
    write_u16_le(&mut buf, t + 36, target_name_offset as u16);
    // CHAP/reverse CHAP fields: all zeros

    // --- String Heap ---
    buf[initiator_name_offset..initiator_name_offset + initiator_name.len()]
        .copy_from_slice(initiator_name);
    buf[target_name_offset..target_name_offset + target_name.len()].copy_from_slice(target_name);

    // ACPI checksum: byte 9 such that the sum of all bytes is 0 mod 256
    let sum: u8 = buf.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    buf[9] = 0u8.wrapping_sub(sum);

    buf
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_ibft_checksum() {
        let table = build_ibft(
            "iqn.2024-01.com.meta.vmtest:initiator",
            &IbftTarget {
                iqn: "iqn.2024-01.com.meta.vmtest:disk1".to_string(),
                portal_ip: "fd00::1".parse().expect("valid IPv6"),
                portal_port: 3260,
                lun: 1,
            },
            &IbftNic {
                ip: "fd00::2".parse().expect("valid IPv6"),
                prefix: 64,
                gateway: "fd00::1".parse().expect("valid IPv6"),
                mac: [0, 0, 0, 0, 0, 1],
            },
        );
        let sum: u8 = table.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0, "ACPI checksum must be zero");
    }

    #[test]
    fn test_ibft_signature_and_offsets() {
        let table = build_ibft(
            "iqn.2024-01.com.meta.vmtest:initiator",
            &IbftTarget {
                iqn: "iqn.2024-01.com.meta.vmtest:disk1".to_string(),
                portal_ip: "fd00::1".parse().expect("valid IPv6"),
                portal_port: 3260,
                lun: 1,
            },
            &IbftNic {
                ip: "fd00::2".parse().expect("valid IPv6"),
                prefix: 64,
                gateway: "fd00::1".parse().expect("valid IPv6"),
                mac: [0, 0, 0, 0, 0, 1],
            },
        );
        assert_eq!(&table[0..4], b"iBFT");
        // The kernel reads the Control structure at `sizeof(acpi_table_ibft)` ==
        // 48 (36-byte ACPI header + 12 reserved). It must land exactly there or
        // `iscsi_ibft` rejects the table with -ENODEV.
        assert_eq!(
            CONTROL_OFFSET, 48,
            "control structure must start at offset 48"
        );
        assert_eq!(table[CONTROL_OFFSET], 1, "control structure ID");
        assert_eq!(table[INITIATOR_OFFSET], 2, "initiator structure ID");
        assert_eq!(table[NIC_OFFSET], 3, "NIC structure ID");
        assert_eq!(table[TARGET_OFFSET], 4, "target structure ID");
    }
}
