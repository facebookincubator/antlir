/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use slog::{error, info, o, Logger};
use structopt::StructOpt;

use crate::kernel_cmdline::{MetalosCmdline, Root};
use kernel_cmdline::KernelCmdArgs;
use net_utils::get_mac;
use systemd::render::{MountSection, NetworkUnit, NetworkUnitMatchSection, UnitSection};
use systemd_generator_lib::{
    materialize_boot_info, Dropin, Environment, ExtraDependencies, ExtraDependency, MountUnit,
    ROOTDISK_MOUNT_SERVICE,
};

// WARNING: keep in sync with the bzl/TARGETS file unit
const ETH_NETWORK_UNIT_FILENAME: &str = "50-eth.network";

#[derive(StructOpt)]
#[cfg_attr(test, derive(Clone))]
pub struct Opts {
    normal_dir: PathBuf,
    #[allow(unused)]
    early_dir: PathBuf,
    #[allow(unused)]
    late_dir: PathBuf,

    /// What directory to place the environment file in.
    #[structopt(default_value = "/run/systemd/generator/")]
    environment_dir: PathBuf,

    /// What directory to place the network unit dropin/override in
    #[structopt(default_value = "/usr/lib/systemd/network/")]
    network_unit_dir: PathBuf,
}

#[derive(Debug, PartialEq)]
pub enum BootMode {
    // We are running metalos but DON'T want to reimage the disk.
    MetalOSExisting,
    // We want to reimage the disk and then setup metalos
    MetalOSReimage,
    // We are booting in legacy mode, no metalos at all
    Legacy,
}

#[derive(Serialize, Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, PartialOrd))]
struct LegacyEnvironment {}
impl Environment for LegacyEnvironment {}

#[derive(Serialize, Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, PartialOrd))]
struct MetalosEnvironment {
    #[serde(rename = "HOST_CONFIG_URI")]
    host_config_uri: String,

    // ROOTDISK_DIR is the absolute path to the location where the initrd
    // will/has mounted the root disk specified on the kernel parameters.
    #[serde(rename = "ROOTDISK_DIR")]
    rootdisk_dir: PathBuf,

    // METALOS_BOOTS_DIR is the directory that contains all of the boot instances
    // but is not a specific boot instance itself
    #[serde(rename = "METALOS_BOOTS_DIR")]
    metalos_boots_dir: PathBuf,

    // METALOS_CURRENT_BOOT_DIR is the directory that we are currently building up
    // for this boot specifically. It will always be beneath METALOS_BOOTS_DIR.
    #[serde(rename = "METALOS_CURRENT_BOOT_DIR")]
    metalos_current_boot_dir: PathBuf,

    // METALOS_IMAGES_DIR is the directory containing all the different image types
    // that metalos can download
    #[serde(rename = "METALOS_IMAGES_DIR")]
    metalos_images_dir: PathBuf,
}
impl Environment for MetalosEnvironment {}

#[derive(Serialize, Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, PartialOrd))]
struct MetalosReimageEnvironment {
    #[serde(flatten)]
    metalos_common: MetalosEnvironment,

    // METALOS_DISK_IMAGE_PKG is the package name and version that should be
    // downloaded and used to image the root disk for this boot.
    #[serde(rename = "METALOS_DISK_IMAGE_PKG")]
    disk_image_package: String,
}
impl Environment for MetalosReimageEnvironment {}

fn make_mount_unit(root: Root, rootdisk: &Path) -> Result<MountUnit> {
    if let Some(what) = &root.root {
        if what.starts_with("LABEL=") {
            Ok(MountUnit {
                unit_section: UnitSection {
                    ..Default::default()
                },
                mount_section: MountSection {
                    what: what.into(),
                    where_: rootdisk.to_path_buf(),
                    options: root.join_flags(),
                    type_: root.fstype,
                },
            })
        } else {
            Err(anyhow!(
                "Not writing run-fs-control.mount root (\"{}\") doesn't start with LABEL=",
                what
            ))
        }
    } else {
        Err(anyhow!(
            "Not writing run-fs-control.mount because no root kernel parameter was provided"
        ))
    }
}

fn make_network_unit_dropin(
    target: String,
    name: String,
    mac_address: Option<String>,
    dropin_filename: String,
) -> Option<Dropin<NetworkUnit>> {
    mac_address.map(|mac| Dropin {
        target: target.into(),
        unit: NetworkUnit {
            match_section: NetworkUnitMatchSection {
                name,
                mac_address: mac,
            },
        },
        dropin_filename: Some(dropin_filename),
    })
}

fn get_boot_id() -> Result<String> {
    let content = std::fs::read_to_string(Path::new("/proc/sys/kernel/random/boot_id"))
        .context("Can't read /proc/sys/kernel/random/boot_id")?;

    Ok(content.trim().replace("-", ""))
}

struct BootInfoResult<ENV: Environment> {
    env: ENV,
    extra_deps: ExtraDependencies,
    mount_unit: MountUnit,
    network_unit_dropin: Option<Dropin<NetworkUnit>>,
}

fn metalos_existing_boot_info(
    root: Root,
    host_config_uri: String,
    mac_address: Option<String>,
) -> Result<BootInfoResult<MetalosEnvironment>> {
    let rootdisk: &Path = metalos_paths::control();
    let boot_id = get_boot_id().context("Failed to get boot id")?;
    let env = MetalosEnvironment {
        host_config_uri,
        rootdisk_dir: metalos_paths::control().into(),
        metalos_boots_dir: rootdisk.join("run/boot"),
        metalos_current_boot_dir: rootdisk.join(format!("run/boot/{}:{}", 0, boot_id)),
        metalos_images_dir: rootdisk.join("image"),
    };

    let extra_deps: ExtraDependencies = vec![
        // This is the main link into the whole metalos flow. The snapshot
        // target needs to download images and things in order to work so it
        // will pull in everything it needs to get the root read for switch
        // root
        (
            "metalos_boot".to_string(),
            ExtraDependency {
                source: "metalos-switch-root.service".into(),
                requires: "metalos-snapshot-root.service".into(),
            },
        ),
        // We also need to make sure the host config is applied correctly before
        // we switch into it.
        (
            "apply_host_config".to_string(),
            ExtraDependency {
                source: "metalos-switch-root.service".into(),
                requires: "metalos-apply-host-config.service".into(),
            },
        ),
    ];

    let mount_unit = make_mount_unit(root, rootdisk).context("Failed to build mount unit")?;
    let network_unit_dropin = make_network_unit_dropin(
        ETH_NETWORK_UNIT_FILENAME.to_string(),
        "eth*".to_string(),
        mac_address,
        "match.conf".to_string(),
    );

    Ok(BootInfoResult {
        env,
        extra_deps,
        mount_unit,
        network_unit_dropin,
    })
}

fn metalos_reimage_boot_info(
    root: Root,
    host_config_uri: String,
    disk_image_package: String,
    mac_address: Option<String>,
) -> Result<BootInfoResult<MetalosReimageEnvironment>> {
    let boot_info_result = metalos_existing_boot_info(root, host_config_uri, mac_address)
        .context("failed to get base info for existing boot")?;

    let mut new_boot_info_result = BootInfoResult {
        env: MetalosReimageEnvironment {
            metalos_common: boot_info_result.env,
            disk_image_package,
        },
        extra_deps: boot_info_result.extra_deps,
        mount_unit: boot_info_result.mount_unit,
        network_unit_dropin: boot_info_result.network_unit_dropin,
    };

    // For reimage we need to insert the image service just before we
    // mount the root disk.
    new_boot_info_result.extra_deps.push((
        "metalos_reimage_boot".to_string(),
        ExtraDependency {
            source: ROOTDISK_MOUNT_SERVICE.into(),
            requires: "metalos-image-root-disk.service".into(),
        },
    ));

    Ok(new_boot_info_result)
}

fn legacy_boot_info(
    root: Root,
    mac_address: Option<String>,
) -> Result<BootInfoResult<LegacyEnvironment>> {
    Ok(BootInfoResult {
        env: LegacyEnvironment {},
        extra_deps: ExtraDependencies::new(),
        mount_unit: make_mount_unit(root, metalos_paths::control())
            .context("Failed to build mount unit")?,
        network_unit_dropin: make_network_unit_dropin(
            ETH_NETWORK_UNIT_FILENAME.to_string(),
            "eth*".to_string(),
            mac_address,
            "match.conf".to_string(),
        ),
    })
}

fn get_initrd_break_dep(cmdline: &MetalosCmdline) -> ExtraDependencies {
    // Transform the `initrd.break` argument into a dependency. An
    // `initrd.break` directive without an argument will default to
    // "initrd.target".
    let mut extra_deps = ExtraDependencies::new();
    if let Some(break_tgt) = &cmdline.initrd_break {
        let break_tgt = match break_tgt {
            None => "initrd.target",
            Some(v) => v.as_str(),
        };
        extra_deps.push((
            "wait-for-debug-shell".to_string(),
            ExtraDependency {
                source: break_tgt.into(),
                requires: "debug-shell.service".into(),
            },
        ));
    };
    extra_deps
}

fn generator_maybe_err(cmdline: MetalosCmdline, log: Logger, opts: Opts) -> Result<BootMode> {
    let boot_mode = detect_mode(&cmdline).context("failed to detect boot mode")?;
    info!(log, "Booting with mode: {:?}", boot_mode);

    // use the mac address in the kernel command line, otherwise try to infer it
    // using crate::net_utils::get_mac()
    let mac_address: Option<String> = match cmdline.mac_address {
        Some(ref mac) => Some(mac.clone()),
        None => match get_mac() {
            Ok(m) => Some(m),
            Err(error) => {
                error!(log, "Skipping network dropin. Error was: {}", error);
                None
            }
        },
    };

    let mut extra_deps = get_initrd_break_dep(&cmdline);
    match &boot_mode {
        BootMode::MetalOSExisting => {
            let boot_info_result = metalos_existing_boot_info(
                cmdline.root,
                cmdline
                    .host_config_uri
                    .context("host-config-uri must be provided for metalos boots")?,
                mac_address,
            )
            .context("Failed to build normal metalos info")?;

            extra_deps.extend(boot_info_result.extra_deps);
            materialize_boot_info(
                log,
                &opts.normal_dir,
                &opts.environment_dir,
                &opts.network_unit_dir,
                boot_info_result.env,
                extra_deps,
                boot_info_result.mount_unit,
                boot_info_result.network_unit_dropin,
            )
        }
        BootMode::MetalOSReimage => {
            let boot_info_result = metalos_reimage_boot_info(
                cmdline.root,
                cmdline
                    .host_config_uri
                    .context("host-config-uri must be provided for metalos boots")?,
                cmdline
                    .root_disk_package
                    .context("Root disk package must be provided for metalos reimage boots")?,
                mac_address,
            )
            .context("Failed to build normal metalos info")?;

            extra_deps.extend(boot_info_result.extra_deps);
            materialize_boot_info(
                log,
                &opts.normal_dir,
                &opts.environment_dir,
                &opts.network_unit_dir,
                boot_info_result.env,
                extra_deps,
                boot_info_result.mount_unit,
                boot_info_result.network_unit_dropin,
            )
        }
        BootMode::Legacy => {
            let boot_info_result = legacy_boot_info(cmdline.root, mac_address)
                .context("failed to build legacy info")?;

            extra_deps.extend(boot_info_result.extra_deps);
            materialize_boot_info(
                log,
                &opts.normal_dir,
                &opts.environment_dir,
                &opts.network_unit_dir,
                boot_info_result.env,
                extra_deps,
                boot_info_result.mount_unit,
                boot_info_result.network_unit_dropin,
            )
        }
    }
    .context("Failed to materialize_boot_info")?;

    // IMPORTANT: this MUST be the last thing that the generator does, otherwise
    // any bugs in the generator can be masked and cause future hard-to-diagnose
    // failures
    let default_target_path = opts.early_dir.join("default.target");
    match std::fs::remove_file(&default_target_path) {
        // NotFound error is Ok, others are not
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        x => x,
    }
    .context("while removing original default.target")?;
    symlink("/usr/lib/systemd/system/initrd.target", default_target_path)
        .context("while changing default target to initrd.target")?;

    Ok(boot_mode)
}

/// This functions job is to discover what type of boot we should be doing.
/// We want this to be the only place where this logic lives and we want very little
/// to no branching logic inside of the other generator methods
fn detect_mode(cmdline: &MetalosCmdline) -> Result<BootMode> {
    // If we have been asked to reimage that takes priority over all other things
    if cmdline.root_disk_package.is_some() {
        Ok(BootMode::MetalOSReimage)
    } else if cmdline.host_config_uri.is_some() {
        Ok(BootMode::MetalOSExisting)
    } else {
        Ok(BootMode::Legacy)
    }
}

pub fn generator(log: Logger, opts: Opts) -> Result<()> {
    info!(log, "metalos-generator starting");

    let sublog = log.new(o!());

    let cmdline = match MetalosCmdline::from_proc_cmdline() {
        Ok(c) => Ok(c),
        Err(e) => {
            error!(
                log,
                "invalid kernel cmdline options for MetalOS. error was: `{:?}`", e,
            );
            Err(e)
        }
    }?;

    match generator_maybe_err(cmdline, sublog, opts) {
        Ok(_) => Ok(()),
        Err(e) => {
            error!(log, "{}", e.to_string());
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, bail};
    use maplit::btreemap;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    use kernel_cmdline::KernelCmdArgs;
    use systemd_generator_lib::ENVIRONMENT_FILENAME;

    fn setup_generator_test(name: &'static str) -> Result<(Logger, PathBuf, Opts, String)> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("Failed to get timestamp")?;
        let tmpdir = std::env::temp_dir().join(format!("test_generator_{}_{:?}", name, ts));

        let normal = tmpdir.join("normal");
        let early = tmpdir.join("early");
        let late = tmpdir.join("late");
        let env = tmpdir.join("env");
        let network = tmpdir.join("network");

        std::fs::create_dir(&tmpdir).context("failed to create tmpdir")?;
        std::fs::create_dir(&normal).context("failed to create normal dir")?;
        std::fs::create_dir(&early).context("failed to create early dir")?;
        std::fs::create_dir(&late).context("failed to create late dir")?;
        std::fs::create_dir(&env).context("failed to create env dir")?;
        std::fs::create_dir(&network).context("failed to create network dir")?;

        symlink("emergency.target", early.join("default.target"))?;

        let opts = Opts {
            normal_dir: normal,
            early_dir: early,
            late_dir: late,
            environment_dir: env,
            network_unit_dir: network,
        };

        let boot_id = get_boot_id().context("Failed to get boot id")?;

        Ok((log, tmpdir, opts, boot_id))
    }

    #[derive(Debug)]
    enum GeneratedFile {
        Contents(String),
        SymlinkTo(PathBuf),
    }

    impl From<String> for GeneratedFile {
        fn from(s: String) -> Self {
            Self::Contents(s)
        }
    }

    impl From<&str> for GeneratedFile {
        fn from(s: &str) -> Self {
            s.to_string().into()
        }
    }

    impl From<PathBuf> for GeneratedFile {
        fn from(p: PathBuf) -> Self {
            Self::SymlinkTo(p)
        }
    }

    fn compare_dir_inner(
        base_dir: &Path,
        expected_contents: &mut BTreeMap<PathBuf, GeneratedFile>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(base_dir).context("failed to read base dir")? {
            let entry = entry.context("failed to read next entry from base dir")?;
            let path = entry.path();
            if path.is_dir() {
                compare_dir_inner(&path, expected_contents)
                    .context(format!("Failed to process directory {:?}", path))?;
            } else {
                match expected_contents.remove(&path) {
                    Some(expected_content) => match expected_content {
                        GeneratedFile::Contents(expected_content) => {
                            let content = std::fs::read_to_string(&path)
                                .context(format!("Can't read file {:?}", path))?;

                            if expected_content != content {
                                return Err(anyhow!(
                                    "File contents for {:?} differs from expected:\ncontents: {:?}\nexpected: {:?}\n",
                                    path,
                                    content,
                                    expected_content,
                                ));
                            }
                        }
                        GeneratedFile::SymlinkTo(dst) => {
                            match std::fs::read_link(&path) {
                                Ok(link_dst) => {
                                    if dst != link_dst {
                                        bail!(
                                            "Expected {:?} to link to {:?}, but actually pointed to {:?}",
                                            path,
                                            dst,
                                            link_dst
                                        );
                                    }
                                }
                                Err(e) => bail!(
                                    "Expected {:?} to link to {:?}, but reading the link failed: {:?}",
                                    path,
                                    dst,
                                    e
                                ),
                            };
                        }
                    },
                    None => {
                        return Err(anyhow!(
                            "Found unexpected file {:?} in directory {:?}",
                            entry.path(),
                            base_dir
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn compare_dir(
        base_dir: &Path,
        mut expected_contents: BTreeMap<PathBuf, GeneratedFile>,
    ) -> Result<()> {
        compare_dir_inner(base_dir, &mut expected_contents)?;
        if expected_contents.is_empty() {
            Ok(())
        } else {
            let keys: Vec<PathBuf> = expected_contents.into_iter().map(|(k, _)| k).collect();
            Err(anyhow!(
                "At least one file not found in {:?}: {:?}",
                base_dir,
                keys
            ))
        }
    }

    #[test]
    fn test_generator_metalos_reimage() -> Result<()> {
        let (log, tmpdir, opts, boot_id) =
            setup_generator_test("metalos_reimage").context("failed to setup test environment")?;

        let cmdline = MetalosCmdline::from_kernel_args(
            "\
            metalos.host-config-uri=\"https://server:8000/config\" \
            metalos.write_root_disk_package=\"reimage_pkg\" \
            rootfstype=btrfs \
            root=LABEL=unittest \
            macaddress=11:22:33:44:55:66\
            ",
        )?;

        let boot_mode =
            generator_maybe_err(cmdline, log, opts.clone()).context("failed to run generator")?;

        assert_eq!(boot_mode, BootMode::MetalOSReimage);

        compare_dir(
            &tmpdir,
            btreemap! {
                opts.normal_dir.join(ROOTDISK_MOUNT_SERVICE) => "\
                    [Unit]\n\
                    [Mount]\n\
                    What=LABEL=unittest\n\
                    Where=/run/fs/control\n\
                    Options=\n\
                    Type=btrfs\n\
                ".into(),
                opts.normal_dir.join("metalos-switch-root.service.d/metalos_boot.conf") => "\
                    [Unit]\n\
                    After=metalos-snapshot-root.service\n\
                    Requires=metalos-snapshot-root.service\n\
                    ".into(),
                opts.normal_dir.join("metalos-switch-root.service.d/apply_host_config.conf") => "\
                    [Unit]\n\
                    After=metalos-apply-host-config.service\n\
                    Requires=metalos-apply-host-config.service\n\
                    ".into(),
                opts.normal_dir.join("run-fs-control.mount.d/metalos_reimage_boot.conf") => "\
                    [Unit]\n\
                    After=metalos-image-root-disk.service\n\
                    Requires=metalos-image-root-disk.service\n\
                    ".into(),
                opts.environment_dir.join(ENVIRONMENT_FILENAME) => format!("\
                    HOST_CONFIG_URI=https://server:8000/config\n\
                    METALOS_BOOTS_DIR=/run/fs/control/run/boot\n\
                    METALOS_CURRENT_BOOT_DIR=/run/fs/control/run/boot/0:{}\n\
                    METALOS_DISK_IMAGE_PKG=reimage_pkg\n\
                    METALOS_IMAGES_DIR=/run/fs/control/image\n\
                    ROOTDISK_DIR=/run/fs/control\n\
                    ",
                    boot_id
                ).into(),
                opts.network_unit_dir.join("50-eth.network.d/match.conf") => "\
                    [Match]\n\
                    Name=eth*\n\
                    MACAddress=11:22:33:44:55:66\n\
                    ".into(),
                opts.early_dir.join("default.target") => PathBuf::from("/usr/lib/systemd/system/initrd.target").into(),
            },
        )
        .context("Failed to ensure tmpdir is setup correctly")?;

        Ok(())
    }

    #[test]
    fn test_generator_metalos_existing() -> Result<()> {
        let (log, tmpdir, opts, boot_id) =
            setup_generator_test("metalos_existing").context("failed to setup test environment")?;

        let cmdline = MetalosCmdline::from_kernel_args(
            "\
            metalos.host-config-uri=\"https://server:8000/config\" \
            rootfstype=btrfs \
            root=LABEL=unittest \
            macaddress=11:22:33:44:55:66\
            ",
        )?;

        let boot_mode =
            generator_maybe_err(cmdline, log, opts.clone()).context("failed to run generator")?;

        assert_eq!(boot_mode, BootMode::MetalOSExisting);

        compare_dir(
            &tmpdir,
            btreemap! {
                opts.normal_dir.join(ROOTDISK_MOUNT_SERVICE) => "\
                    [Unit]\n\
                    [Mount]\n\
                    What=LABEL=unittest\n\
                    Where=/run/fs/control\n\
                    Options=\n\
                    Type=btrfs\n\
                ".into(),
                opts.normal_dir.join("metalos-switch-root.service.d/metalos_boot.conf") => "\
                    [Unit]\n\
                    After=metalos-snapshot-root.service\n\
                    Requires=metalos-snapshot-root.service\n\
                    ".into(),
                opts.normal_dir.join("metalos-switch-root.service.d/apply_host_config.conf") => "\
                    [Unit]\n\
                    After=metalos-apply-host-config.service\n\
                    Requires=metalos-apply-host-config.service\n\
                    ".into(),
                opts.environment_dir.join(ENVIRONMENT_FILENAME) => format!("\
                    HOST_CONFIG_URI=https://server:8000/config\n\
                    METALOS_BOOTS_DIR=/run/fs/control/run/boot\n\
                    METALOS_CURRENT_BOOT_DIR=/run/fs/control/run/boot/0:{}\n\
                    METALOS_IMAGES_DIR=/run/fs/control/image\n\
                    ROOTDISK_DIR=/run/fs/control\n\
                    ",
                    boot_id
                ).into(),
                opts.network_unit_dir.join("50-eth.network.d/match.conf") => "\
                    [Match]\n\
                    Name=eth*\n\
                    MACAddress=11:22:33:44:55:66\n\
                    ".into(),
                opts.early_dir.join("default.target") => PathBuf::from("/usr/lib/systemd/system/initrd.target").into(),
            },
        )
        .context("Failed to ensure tmpdir is setup correctly")?;

        Ok(())
    }

    #[test]
    fn test_generator_fail() -> Result<()> {
        let (log, tmpdir, opts, _) =
            setup_generator_test("generator_fail").context("failed to setup test environment")?;

        let cmdline = MetalosCmdline::from_kernel_args("")?;

        assert!(generator_maybe_err(cmdline, log, opts.clone()).is_err());

        compare_dir(
            &tmpdir,
            btreemap! {
                opts.early_dir.join("default.target") => PathBuf::from("emergency.target").into(),
            },
        )
        .context("Failed to ensure tmpdir is setup correctly")?;

        Ok(())
    }

    #[test]
    fn test_generator_legacy() -> Result<()> {
        let (log, tmpdir, opts, _) =
            setup_generator_test("legacy").context("failed to setup test environment")?;

        let cmdline = MetalosCmdline::from_kernel_args(
            "\
            root=LABEL=unittest \
            rootflags=f1,f2,f3 \
            ro \
            macaddress=11:22:33:44:55:66\
            ",
        )?;

        let boot_mode =
            generator_maybe_err(cmdline, log, opts.clone()).context("failed to run generator")?;

        assert_eq!(boot_mode, BootMode::Legacy);

        compare_dir(
            &tmpdir,
            btreemap! {
                opts.normal_dir.join(ROOTDISK_MOUNT_SERVICE) => "\
                    [Unit]\n\
                    [Mount]\n\
                    What=LABEL=unittest\n\
                    Where=/run/fs/control\n\
                    Options=f1,f2,f3,ro\n\
                ".into(),
                opts.environment_dir.join(ENVIRONMENT_FILENAME) => "".into(),
                opts.network_unit_dir.join("50-eth.network.d/match.conf") => "\
                    [Match]\n\
                    Name=eth*\n\
                    MACAddress=11:22:33:44:55:66\n\
                    ".into(),
                opts.early_dir.join("default.target") => PathBuf::from("/usr/lib/systemd/system/initrd.target").into(),
            },
        )
        .context("Failed to ensure tmpdir is setup correctly")?;

        Ok(())
    }

    #[test]
    fn test_metalos_reimage_boot_info() -> Result<()> {
        let boot_info_result = metalos_reimage_boot_info(
            Root {
                root: Some("LABEL=unittest".to_string()),
                fstype: Some("testfs".to_string()),
                flags: Some(vec!["f1".to_string(), "f2".to_string(), "f3".to_string()]),
                ro: false,
                rw: true,
            },
            "test_config_uri".to_string(),
            "test_reimage_package:123".to_string(),
            Some("11:22:33:44:55:66".to_string()),
        )
        .context("failed to get boot info")?;

        let boot_id = get_boot_id().context("failed to get boot id")?;

        assert_eq!(
            boot_info_result.env,
            MetalosReimageEnvironment {
                metalos_common: MetalosEnvironment {
                    host_config_uri: "test_config_uri".to_string(),
                    rootdisk_dir: "/run/fs/control".into(),
                    metalos_boots_dir: "/run/fs/control/run/boot".into(),
                    metalos_current_boot_dir: format!("/run/fs/control/run/boot/0:{}", boot_id)
                        .into(),
                    metalos_images_dir: "/run/fs/control/image".into(),
                },
                disk_image_package: "test_reimage_package:123".to_string(),
            }
        );

        assert_eq!(
            boot_info_result.extra_deps,
            vec![
                (
                    "metalos_boot".to_string(),
                    ExtraDependency {
                        source: "metalos-switch-root.service".into(),
                        requires: "metalos-snapshot-root.service".into(),
                    }
                ),
                (
                    "apply_host_config".to_string(),
                    ExtraDependency {
                        source: "metalos-switch-root.service".into(),
                        requires: "metalos-apply-host-config.service".into(),
                    }
                ),
                (
                    "metalos_reimage_boot".to_string(),
                    ExtraDependency {
                        source: "run-fs-control.mount".into(),
                        requires: "metalos-image-root-disk.service".into(),
                    }
                ),
            ]
        );

        assert_eq!(
            boot_info_result.mount_unit,
            MountUnit {
                unit_section: UnitSection::default(),
                mount_section: MountSection {
                    what: "LABEL=unittest".into(),
                    where_: "/run/fs/control".into(),
                    options: Some("f1,f2,f3,rw".to_string()),
                    type_: Some("testfs".to_string()),
                }
            }
        );

        assert_eq!(
            boot_info_result.network_unit_dropin,
            Some(Dropin {
                target: ETH_NETWORK_UNIT_FILENAME.into(),
                unit: NetworkUnit {
                    match_section: NetworkUnitMatchSection {
                        name: "eth*".to_string(),
                        mac_address: "11:22:33:44:55:66".to_string()
                    },
                },
                dropin_filename: Some("match.conf".to_string()),
            })
        );

        Ok(())
    }

    #[test]
    fn test_metalos_existing_boot_info() -> Result<()> {
        let boot_info_result = metalos_existing_boot_info(
            Root {
                root: Some("LABEL=unittest".to_string()),
                fstype: Some("testfs".to_string()),
                flags: Some(vec!["f1".to_string(), "f2".to_string(), "f3".to_string()]),
                ro: false,
                rw: true,
            },
            "test_config_uri".to_string(),
            Some("11:22:33:44:55:66".to_string()),
        )
        .context("failed to get boot info")?;

        let boot_id = get_boot_id().context("failed to get boot id")?;

        assert_eq!(
            boot_info_result.env,
            MetalosEnvironment {
                host_config_uri: "test_config_uri".to_string(),
                rootdisk_dir: "/run/fs/control".into(),
                metalos_boots_dir: "/run/fs/control/run/boot".into(),
                metalos_current_boot_dir: format!("/run/fs/control/run/boot/0:{}", boot_id).into(),
                metalos_images_dir: "/run/fs/control/image".into(),
            }
        );

        assert_eq!(
            boot_info_result.extra_deps,
            vec![
                (
                    "metalos_boot".to_string(),
                    ExtraDependency {
                        source: "metalos-switch-root.service".into(),
                        requires: "metalos-snapshot-root.service".into(),
                    }
                ),
                (
                    "apply_host_config".to_string(),
                    ExtraDependency {
                        source: "metalos-switch-root.service".into(),
                        requires: "metalos-apply-host-config.service".into(),
                    }
                ),
            ]
        );

        assert_eq!(
            boot_info_result.mount_unit,
            MountUnit {
                unit_section: UnitSection::default(),
                mount_section: MountSection {
                    what: "LABEL=unittest".into(),
                    where_: "/run/fs/control".into(),
                    options: Some("f1,f2,f3,rw".to_string()),
                    type_: Some("testfs".to_string()),
                }
            }
        );

        assert_eq!(
            boot_info_result.network_unit_dropin,
            Some(Dropin {
                target: ETH_NETWORK_UNIT_FILENAME.into(),
                unit: NetworkUnit {
                    match_section: NetworkUnitMatchSection {
                        name: "eth*".to_string(),
                        mac_address: "11:22:33:44:55:66".to_string()
                    },
                },
                dropin_filename: Some("match.conf".to_string()),
            })
        );

        Ok(())
    }

    #[test]
    fn test_legacy_boot_info() -> Result<()> {
        let boot_info_result = legacy_boot_info(
            Root {
                root: Some("LABEL=unittest".to_string()),
                fstype: Some("testfs".to_string()),
                flags: Some(vec!["f1".to_string(), "f2".to_string(), "f3".to_string()]),
                ro: false,
                rw: true,
            },
            Some("11:22:33:44:55:66".to_string()),
        )
        .context("failed to get boot info")?;

        assert_eq!(boot_info_result.env, LegacyEnvironment {});
        assert_eq!(boot_info_result.extra_deps, ExtraDependencies::new());

        assert_eq!(
            boot_info_result.mount_unit,
            MountUnit {
                unit_section: UnitSection::default(),
                mount_section: MountSection {
                    what: "LABEL=unittest".into(),
                    where_: "/run/fs/control".into(),
                    options: Some("f1,f2,f3,rw".to_string()),
                    type_: Some("testfs".to_string()),
                }
            }
        );

        assert_eq!(
            boot_info_result.network_unit_dropin,
            Some(Dropin {
                target: ETH_NETWORK_UNIT_FILENAME.into(),
                unit: NetworkUnit {
                    match_section: NetworkUnitMatchSection {
                        name: "eth*".to_string(),
                        mac_address: "11:22:33:44:55:66".to_string()
                    },
                },
                dropin_filename: Some("match.conf".to_string()),
            })
        );

        Ok(())
    }

    #[containertest]
    fn test_get_boot_id() -> Result<()> {
        let output = std::process::Command::new("journalctl")
            .arg("--list-boots")
            .output()
            .context("Failed to run journalctl --list-boots")?;

        if !output.status.success() {
            return Err(anyhow!("journalctl command filed: {:?}", output));
        }

        let stdout =
            std::str::from_utf8(&output.stdout).context("Failed to convert stdout to str")?;

        for line in stdout.lines() {
            let line = line.trim();
            println!("line: '{}'", line);
            if line.starts_with("0 ") {
                let (_, line) = line.split_once(" ").context("boot entry had no space")?;
                let (boot_id, _) = line
                    .split_once(" ")
                    .context("second half of boot entry has no space")?;

                assert_eq!(get_boot_id().context("Failed to get boot id")?, boot_id,);

                return Ok(());
            }
        }

        Err(anyhow!("Unable to find bootid from journalctl: {}", stdout))
    }

    #[test]
    fn test_make_mount_unit() -> Result<()> {
        assert_eq!(
            make_mount_unit(
                Root {
                    root: Some("LABEL=unittest".to_string()),
                    fstype: Some("testfs".to_string()),
                    flags: Some(vec!["f1".to_string(), "f2".to_string(), "f3".to_string()]),
                    ro: false,
                    rw: true,
                },
                Path::new("/test_rootdisk"),
            )
            .context("failed to write moot disk")?,
            MountUnit {
                unit_section: UnitSection::default(),
                mount_section: MountSection {
                    what: "LABEL=unittest".into(),
                    where_: "/test_rootdisk".into(),
                    options: Some("f1,f2,f3,rw".to_string()),
                    type_: Some("testfs".to_string()),
                }
            }
        );
        assert_eq!(
            make_mount_unit(
                Root {
                    root: Some("LABEL=unittest".to_string()),
                    fstype: None,
                    flags: None,
                    ro: true,
                    rw: false,
                },
                Path::new("/test_rootdisk"),
            )
            .context("failed to write moot disk")?,
            MountUnit {
                unit_section: UnitSection::default(),
                mount_section: MountSection {
                    what: "LABEL=unittest".into(),
                    where_: "/test_rootdisk".into(),
                    options: Some("ro".to_string()),
                    type_: None,
                }
            }
        );

        assert!(
            make_mount_unit(
                Root {
                    root: None,
                    fstype: None,
                    flags: None,
                    ro: true,
                    rw: false,
                },
                Path::new("/test_rootdisk"),
            )
            .is_err()
        );

        assert!(
            make_mount_unit(
                Root {
                    root: Some("/dev/vda".into()),
                    fstype: None,
                    flags: None,
                    ro: true,
                    rw: false,
                },
                Path::new("/test_rootdisk"),
            )
            .is_err()
        );
        Ok(())
    }
}
