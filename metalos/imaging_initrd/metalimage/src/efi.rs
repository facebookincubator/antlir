use std::collections::HashMap;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use metalos_disk::DiskDevPath;
use metalos_host_configs::boot_config::Bootloader;
use slog::info;
use slog::Logger;

pub(crate) static BOOTLOADER_FILENAME: &str = "metalos.efi";

macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).expect("regex did not compile"))
    }};
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BootEntry {
    Network(String, Bootnum),
    Metalos(Bootnum),
    Other(String, Bootnum),
}

impl BootEntry {
    fn num(&self) -> Bootnum {
        match self {
            Self::Network(_, num) | Self::Metalos(num) | Self::Other(_, num) => num.clone(),
        }
    }
}

impl FromStr for BootEntry {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        if let Some(caps) =
            regex!(r#"^Boot(?P<num>[[:xdigit:]]+)\*?\s+(?P<label>.*)?$"#).captures(s)
        {
            let num = Bootnum(
                caps.name("num")
                    .expect("non-optional capturing group")
                    .as_str()
                    .into(),
            );
            let label = caps
                .name("label")
                .expect("non-optional capturing group")
                .as_str()
                .trim()
                .trim_end_matches('-')
                .trim_end();
            // I hate this, but there is no reliable way to tell if this is a
            // network boot entry just by shelling out to efibootmgr...
            if label.contains("PXE") {
                Ok(Self::Network(label.into(), num))
            } else {
                match label {
                    "MetalOS" => Ok(Self::Metalos(num)),
                    _ => Ok(Self::Other(label.into(), num)),
                }
            }
        } else {
            Err(anyhow!("{} did not match boot entry regex", s))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Bootnum(String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct EfiConfig {
    order: Vec<Bootnum>,
    entries: HashMap<Bootnum, BootEntry>,
}

impl FromStr for EfiConfig {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let order = s
            .lines()
            .filter_map(|l| l.strip_prefix("BootOrder: "))
            .next()
            .with_context(|| format!("missing BootOrder in {:?}", s))?
            .split(',')
            .map(|s| Bootnum(s.to_string()))
            .collect();
        let entries = s
            .lines()
            .filter_map(|l| l.parse().ok())
            .map(|e: BootEntry| (e.num(), e))
            .collect();
        Ok(Self { order, entries })
    }
}

fn current_config() -> Result<EfiConfig> {
    let out = std::process::Command::new("efibootmgr")
        .output()
        .context("while running efibootmgr")?;
    ensure!(
        out.status.success(),
        "efibootmgr failed with exit code {} (stderr: {})",
        out.status,
        std::str::from_utf8(&out.stderr).unwrap_or("not-utf8")
    );
    let stdout = std::str::from_utf8(&out.stdout).context("efibootmgr stdout is not utf-8")?;
    stdout.parse()
}

/// Ensure that any existing MetalOS entry is deleted
fn delete_metalos_entry() -> Result<()> {
    let old_config = current_config().context("while loading current efi config")?;
    if let Some(metalos_num) = old_config.entries.into_values().find_map(|e| match e {
        BootEntry::Metalos(n) => Some(n),
        _ => None,
    }) {
        let out = std::process::Command::new("efibootmgr")
            .arg("--delete-bootnum")
            .arg("--bootnum")
            .arg(metalos_num.0)
            .output()
            .context("while running efibootmgr")?;
        ensure!(
            out.status.success(),
            "efibootmgr failed with exit code {} (stderr: {})",
            out.status,
            std::str::from_utf8(&out.stderr).unwrap_or("not-utf8")
        );
    }
    Ok(())
}

fn set_boot_order(order: Vec<Bootnum>) -> Result<()> {
    let order: Vec<String> = order.into_iter().map(|n| n.0).collect();
    let out = std::process::Command::new("efibootmgr")
        .arg("--bootorder")
        .arg(order.join(","))
        .output()
        .context("while running efibootmgr")?;
    ensure!(
        out.status.success(),
        "efibootmgr failed with exit code {} (stderr: {})",
        out.status,
        std::str::from_utf8(&out.stderr).unwrap_or("not-utf8")
    );
    Ok(())
}

/// Add an EFI boot entry to point to MetalOS's bootloader. The bootloader
/// binary does not need to exist yet, it will be installed later in metalimage
/// or during the regular offline-update process.
/// The new MetalOS entry will be set as the second entry in BootOrder, assuming
/// that the first entry looks like a network boot entry.
pub(crate) fn setup_efi_boot(
    log: Logger,
    disk: &DiskDevPath,
    bootloader: &Bootloader,
) -> Result<()> {
    delete_metalos_entry().context("while deleting any existing metalos entries")?;
    let old_config = current_config().context("while loading current efi config")?;
    info!(log, "initial config = {:?}", old_config);
    // basic sanity check, if the first entry is not a network boot I don't know
    // what to do
    ensure!(
        matches!(
            old_config.entries[&old_config.order[0]],
            BootEntry::Network(_, _)
        ),
        "First entry is not a network boot: {:?}",
        old_config,
    );

    let out = std::process::Command::new("efibootmgr")
        .arg("--create")
        .arg("--disk")
        .arg(&disk.0)
        .arg("--label")
        .arg("MetalOS")
        .arg("--loader")
        .arg(format!("\\EFI\\{}", BOOTLOADER_FILENAME))
        .arg("--unicode")
        .arg(&bootloader.cmdline)
        .output()
        .context("while running efibootmgr")?;
    ensure!(
        out.status.success(),
        "efibootmgr failed with exit code {} (stderr: {})",
        out.status,
        std::str::from_utf8(&out.stderr).unwrap_or("not-utf8")
    );

    // after running --create, the previous first entry should be second, so
    // we'll reorder it back so that metalos is second
    let new_config: EfiConfig = std::str::from_utf8(&out.stdout)
        .context("efibootmgr output not utf-8")?
        .parse()
        .context("while parsing efibootmgr output")?;
    info!(log, "intermediate config = {:?}", new_config);
    ensure!(
        matches!(
            new_config.entries[&new_config.order[0]],
            BootEntry::Metalos(_)
        ),
        "MetalOS entry was not first: {:?}",
        new_config,
    );
    ensure!(
        new_config.order[1] == old_config.order[0],
        "Previous first entry is not second: {:?}",
        new_config,
    );

    // now swap the second and first entries
    let mut order = new_config.order;
    order.swap(0, 1);
    info!(log, "setting new bootorder to {:?}", order);
    set_boot_order(order).context("while setting BootOrder")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_entries() -> Result<()> {
        assert_eq!(
            BootEntry::Network(
                "UEFI: PXE IPv6 Mellanox Network Adapter - 98:03:9B:98:6D:6E".into(),
                Bootnum("0003".to_string())
            ),
            "Boot0003* UEFI: PXE IPv6 Mellanox Network Adapter - 98:03:9B:98:6D:6E".parse()?
        );
        assert_eq!(
            BootEntry::Network(
                "UEFI_Slot3_Port0 PXE IPv6 Broadcom NetXtreme-E Single 100Gb OCP 3.0 Ethernet".into(),
                Bootnum("0002".to_string())
            ),
            "Boot0002* UEFI_Slot3_Port0 PXE IPv6 Broadcom NetXtreme-E Single 100Gb OCP 3.0 Ethernet".parse()?,
        );
        assert_eq!(
            BootEntry::Other("Hard Drive".into(), Bootnum("0001".to_string())),
            "Boot0001  Hard Drive".parse()?,
        );
        assert_eq!(
            BootEntry::Metalos(Bootnum("0002".to_string())),
            "Boot0002  MetalOS".parse()?,
        );
        Ok(())
    }

    #[test]
    fn parse_full() -> Result<()> {
        assert_eq!(
            EfiConfig {
                order: vec![
                    Bootnum("0000".into()),
                    Bootnum("0003".into()),
                    Bootnum("0001".into()),
                    Bootnum("0002".into()),
                ],
                entries: maplit::hashmap! {
                    Bootnum("0000".into()) => BootEntry::Metalos(Bootnum("0000".into())),
                    Bootnum("0003".into()) => BootEntry::Network("UEFI: PXE IPv6 Mellanox Network Adapter - 98:03:9B:98:6D:6E".into(), Bootnum("0003".into())),
                    Bootnum("0001".into()) => BootEntry::Other("Hard Drive".into(), Bootnum("0001".into())),
                    Bootnum("0002".into()) => BootEntry::Other("UEFI: Built-in EFI Shell".into(), Bootnum("0002".into())),
                }
            },
            "BootCurrent: 0003\n\
            Timeout: 2 seconds\n\
            BootOrder: 0000,0003,0001,0002\n\
            Boot0000* MetalOS\n\
            Boot0001  Hard Drive\n\
            Boot0002  UEFI: Built-in EFI Shell\n\
            Boot0003* UEFI: PXE IPv6 Mellanox Network Adapter - 98:03:9B:98:6D:6E\n\
            MirroredPercentageAbove4G: 0.00\n\
            MirrorMemoryBelow4GB: false\n"
                .parse()?
        );
        Ok(())
    }
}
