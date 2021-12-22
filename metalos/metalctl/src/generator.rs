/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use find_root_disk::{DiskPath, FindRootDisk, SingleDiskFinder};
use serde::Serialize;
use slog::{error, info, o, Logger};
use structopt::StructOpt;

use crate::kernel_cmdline::{MetalosCmdline, Root};
use crate::switch_root::ROOTDISK_DIR;
use generator_lib::{
    materialize_boot_info, Environment, ExtraDependencies, ExtraDependency, MountUnit,
    ROOTDISK_MOUNT_SERVICE,
};
use systemd::render::{MountSection, UnitSection};

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

    // METALOS_OS_PKG is the package name and version that should be
    // downloaded and used as the base operating system image for this boot
    #[serde(rename = "METALOS_OS_PKG")]
    os_package: String,
}
impl Environment for MetalosEnvironment {}

#[derive(Serialize, Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, PartialOrd))]
struct MetalosReimageEnvironment {
    #[serde(flatten)]
    metalos_common: MetalosEnvironment,

    // ROOTDISK_DEVICE is the device node path to the thing we want to mount
    // as our root disk.
    #[serde(rename = "ROOTDISK_DEVICE")]
    rootdisk_device: PathBuf,

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
                "Not writing rootdisk.mount root (\"{}\") doesn't start with LABEL=",
                what
            ))
        }
    } else {
        Err(anyhow!(
            "Not writing rootdisk.mount because no root kernel parameter was provided"
        ))
    }
}

fn get_boot_id() -> Result<String> {
    let content = std::fs::read_to_string(Path::new("/proc/sys/kernel/random/boot_id"))
        .context("Can't read /proc/sys/kernel/random/boot_id")?;

    Ok(content.trim().replace("-", ""))
}

fn metalos_existing_boot_info(
    root: Root,
    host_config_uri: String,
    os_package: String,
) -> Result<(MetalosEnvironment, ExtraDependencies, MountUnit)> {
    let rootdisk: &Path = Path::new(ROOTDISK_DIR);
    let boot_id = get_boot_id().context("Failed to get boot id")?;
    let env = MetalosEnvironment {
        host_config_uri,
        rootdisk_dir: ROOTDISK_DIR.into(),
        metalos_boots_dir: rootdisk.join("run/boot"),
        metalos_current_boot_dir: rootdisk.join(format!("run/boot/{}:{}", 0, boot_id)),
        metalos_images_dir: rootdisk.join("image"),
        os_package,
    };

    let mut extra_deps = ExtraDependencies::new();

    // This is the main link into the whole metalos flow. The snapshot
    // target needs to download images and things in order to work so it
    // will pull in everything it needs to get the root read for switch
    // root
    extra_deps.insert(
        "metalos_boot".to_string(),
        ExtraDependency {
            source: "metalos-switch-root.service".into(),
            requires: "metalos-snapshot-root.service".into(),
        },
    );

    // We also need to make sure the host config is applied correctly before
    // we switch into it.
    extra_deps.insert(
        "apply_host_config".to_string(),
        ExtraDependency {
            source: "metalos-switch-root.service".into(),
            requires: "metalos-apply-host-config.service".into(),
        },
    );

    let mount_unit = make_mount_unit(root, rootdisk).context("Failed to build mount unit")?;

    Ok((env, extra_deps, mount_unit))
}

fn metalos_reimage_boot_info<FD: FindRootDisk>(
    root: Root,
    host_config_uri: String,
    os_package: String,
    disk_image_package: String,
    disk_finder: &FD,
) -> Result<(MetalosReimageEnvironment, ExtraDependencies, MountUnit)> {
    let (base_env, mut extra_deps, mount_unit) =
        metalos_existing_boot_info(root, host_config_uri, os_package)
            .context("failed to get base info for existing boot")?;

    let root_device = disk_finder
        .get_root_device()
        .context("Failed to find root device to write root_disk_package to")?
        .dev_node()
        .context("Failed to get the devnode for root disk")?;

    let env = MetalosReimageEnvironment {
        metalos_common: base_env,
        rootdisk_device: root_device,
        disk_image_package,
    };

    // For reimage we need to insert the image service just before we
    // mount the root disk.
    extra_deps.insert(
        "metalos_reimage_boot".to_string(),
        ExtraDependency {
            source: ROOTDISK_MOUNT_SERVICE.into(),
            requires: "metalos-image-root-disk.service".into(),
        },
    );

    Ok((env, extra_deps, mount_unit))
}

fn legacy_boot_info(root: Root) -> Result<(LegacyEnvironment, ExtraDependencies, MountUnit)> {
    Ok((
        LegacyEnvironment {},
        ExtraDependencies::new(),
        make_mount_unit(root, Path::new(ROOTDISK_DIR)).context("Failed to build mount unit")?,
    ))
}

fn generator_maybe_err<FD: FindRootDisk>(
    cmdline: MetalosCmdline,
    log: Logger,
    opts: Opts,
    disk_finder: &FD,
) -> Result<BootMode> {
    let boot_mode = detect_mode(&cmdline).context("failed to detect boot mode")?;
    info!(log, "Booting with mode: {:?}", boot_mode);

    match &boot_mode {
        BootMode::MetalOSExisting => {
            let (env, extra_deps, mount_units) = metalos_existing_boot_info(
                cmdline.root,
                cmdline
                    .host_config_uri
                    .context("host-config-uri must be provided for metalos boots")?,
                cmdline
                    .os_package
                    .context("OS package must be provided for metalos boots")?,
            )
            .context("Failed to build normal metalos info")?;

            materialize_boot_info(
                log,
                &opts.normal_dir,
                &opts.environment_dir,
                env,
                extra_deps,
                mount_units,
            )
        }
        BootMode::MetalOSReimage => {
            let (env, extra_deps, mount_units) = metalos_reimage_boot_info(
                cmdline.root,
                cmdline
                    .host_config_uri
                    .context("host-config-uri must be provided for metalos boots")?,
                cmdline
                    .os_package
                    .context("OS package must be provided for metalos boots")?,
                cmdline
                    .root_disk_package
                    .context("Root disk package must be provided for metalos reimage boots")?,
                disk_finder,
            )
            .context("Failed to build normal metalos info")?;

            materialize_boot_info(
                log,
                &opts.normal_dir,
                &opts.environment_dir,
                env,
                extra_deps,
                mount_units,
            )
        }
        BootMode::Legacy => {
            let (env, extra_deps, mount_units) =
                legacy_boot_info(cmdline.root).context("failed to build legacy info")?;

            materialize_boot_info(
                log,
                &opts.normal_dir,
                &opts.environment_dir,
                env,
                extra_deps,
                mount_units,
            )
        }
    }
    .context("Failed to materialize_boot_info")?;

    Ok(boot_mode)
}

/// This functions job is to discover what type of boot we should be doing.
/// We want this to be the only place where this logic lives and we want very little
/// to no branching logic inside of the other generator methods
fn detect_mode(cmdline: &MetalosCmdline) -> Result<BootMode> {
    // If we have been asked to reimage that takes priority over all other things
    if cmdline.root_disk_package.is_some() {
        Ok(BootMode::MetalOSReimage)
    } else if cmdline.os_package.is_some() {
        Ok(BootMode::MetalOSExisting)
    } else {
        Ok(BootMode::Legacy)
    }
}

pub fn generator(log: Logger, opts: Opts) -> Result<()> {
    info!(log, "metalos-generator starting");

    let sublog = log.new(o!());

    let cmdline = match MetalosCmdline::from_kernel() {
        Ok(c) => Ok(c),
        Err(e) => {
            error!(
                log,
                "invalid kernel cmdline options for MetalOS. error was: `{:?}`", e,
            );
            Err(e)
        }
    }?;

    match generator_maybe_err(cmdline, sublog, opts, &SingleDiskFinder::new()) {
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
    use anyhow::anyhow;
    use find_root_disk::DiskDiscovery;
    use maplit::btreemap;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    use generator_lib::ENVIRONMENT_FILENAME;

    #[derive(Clone)]
    struct MockDisk {
        disk: PathBuf,
    }
    impl DiskPath for MockDisk {
        fn dev_node(&self) -> Result<PathBuf> {
            Ok(self.disk.clone())
        }
        fn sys_path(&self) -> &Path {
            panic!("sys_path not implemented for mock");
        }
    }

    struct MockDiskDiscovery {}
    impl DiskDiscovery for MockDiskDiscovery {
        type Output = MockDisk;
        fn discover_devices() -> Result<Vec<Self::Output>> {
            panic!("Discover devices not implemented for mock");
        }
    }

    struct MockDiskFinder {
        disk: MockDisk,
    }
    struct MockErrDiskFinder {}

    impl FindRootDisk for MockDiskFinder {
        type Output = MockDisk;
        type Discovery = MockDiskDiscovery;
        fn get_root_device(&self) -> Result<Self::Output> {
            Ok(self.disk.clone())
        }

        fn find_root_disk(&self, _: Vec<Self::Output>) -> Result<Self::Output> {
            Ok(self.disk.clone())
        }
    }

    impl FindRootDisk for MockErrDiskFinder {
        type Output = MockDisk;
        type Discovery = MockDiskDiscovery;
        fn get_root_device(&self) -> Result<Self::Output> {
            Err(anyhow!("Forced unit test error"))
        }

        fn find_root_disk(&self, _: Vec<Self::Output>) -> Result<Self::Output> {
            Err(anyhow!("Forced unit test error"))
        }
    }

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

        std::fs::create_dir(&tmpdir).context("failed to create tmpdir")?;
        std::fs::create_dir(&normal).context("failed to create normal dir")?;
        std::fs::create_dir(&early).context("failed to create early dir")?;
        std::fs::create_dir(&late).context("failed to create late dir")?;
        std::fs::create_dir(&env).context("failed to create env dir")?;

        let opts = Opts {
            normal_dir: normal,
            early_dir: early,
            late_dir: late,
            environment_dir: env,
        };

        let boot_id = get_boot_id().context("Failed to get boot id")?;

        Ok((log, tmpdir, opts, boot_id))
    }

    fn compare_dir_inner(
        base_dir: &Path,
        expected_contents: &mut BTreeMap<PathBuf, String>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(base_dir).context("failed to read base dir")? {
            let entry = entry.context("failed to read next entry from base dir")?;
            let path = entry.path();
            if path.is_dir() {
                compare_dir_inner(&path, expected_contents)
                    .context(format!("Failed to process directory {:?}", path))?;
            } else {
                match expected_contents.remove(&path) {
                    Some(expected_content) => {
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
        mut expected_contents: BTreeMap<PathBuf, String>,
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

        let cmdline: MetalosCmdline = "\
            metalos.host-config-uri=\"https://server:8000/config\" \
            metalos.write_root_disk_package=\"reimage_pkg\" \
            metalos.os_package=\"somePackage\" \
            rootfstype=btrfs \
            root=LABEL=unittest\
            "
        .parse()?;

        let disk = MockDisk {
            disk: "/dev/unittest".into(),
        };

        let boot_mode = generator_maybe_err(cmdline, log, opts.clone(), &MockDiskFinder { disk })
            .context("failed to run generator")?;

        assert_eq!(boot_mode, BootMode::MetalOSReimage);

        compare_dir(
            &tmpdir,
            btreemap! {
                opts.normal_dir.join(ROOTDISK_MOUNT_SERVICE) => "\
                    [Unit]\n\
                    [Mount]\n\
                    What=LABEL=unittest\n\
                    Where=/rootdisk\n\
                    Options=\n\
                    Type=btrfs\n\
                ".to_string(),
                opts.normal_dir.join("metalos-switch-root.service.d/metalos_boot.conf") => "\
                    [Unit]\n\
                    After=metalos-snapshot-root.service\n\
                    Requires=metalos-snapshot-root.service\n\
                    ".to_string(),
                opts.normal_dir.join("metalos-switch-root.service.d/apply_host_config.conf") => "\
                    [Unit]\n\
                    After=metalos-apply-host-config.service\n\
                    Requires=metalos-apply-host-config.service\n\
                    ".to_string(),
                opts.normal_dir.join("rootdisk.mount.d/metalos_reimage_boot.conf") => "\
                    [Unit]\n\
                    After=metalos-image-root-disk.service\n\
                    Requires=metalos-image-root-disk.service\n\
                    ".to_string(),
                opts.environment_dir.join(ENVIRONMENT_FILENAME) => format!("\
                    HOST_CONFIG_URI=https://server:8000/config\n\
                    METALOS_BOOTS_DIR=/rootdisk/run/boot\n\
                    METALOS_CURRENT_BOOT_DIR=/rootdisk/run/boot/0:{}\n\
                    METALOS_DISK_IMAGE_PKG=reimage_pkg\n\
                    METALOS_IMAGES_DIR=/rootdisk/image\n\
                    METALOS_OS_PKG=somePackage\n\
                    ROOTDISK_DEVICE=/dev/unittest\n\
                    ROOTDISK_DIR=/rootdisk\n\
                    ",
                    boot_id
                )
            },
        )
        .context("Failed to ensure tmpdir is setup correctly")?;

        Ok(())
    }

    #[test]
    fn test_generator_metalos_existing() -> Result<()> {
        let (log, tmpdir, opts, boot_id) =
            setup_generator_test("metalos_existing").context("failed to setup test environment")?;

        let cmdline: MetalosCmdline = "\
            metalos.host-config-uri=\"https://server:8000/config\" \
            metalos.os_package=\"somePackage\" \
            rootfstype=btrfs \
            root=LABEL=unittest\
            "
        .parse()?;

        let boot_mode = generator_maybe_err(
            cmdline,
            log,
            opts.clone(),
            // We want to enforce that metalos setup doesn't use the disk searching functionality
            // unless we are asking to reimage. It should be using the lables only otherwise
            &MockErrDiskFinder {},
        )
        .context("failed to run generator")?;

        assert_eq!(boot_mode, BootMode::MetalOSExisting);

        compare_dir(
            &tmpdir,
            btreemap! {
                opts.normal_dir.join(ROOTDISK_MOUNT_SERVICE) => "\
                    [Unit]\n\
                    [Mount]\n\
                    What=LABEL=unittest\n\
                    Where=/rootdisk\n\
                    Options=\n\
                    Type=btrfs\n\
                ".to_string(),
                opts.normal_dir.join("metalos-switch-root.service.d/metalos_boot.conf") => "\
                    [Unit]\n\
                    After=metalos-snapshot-root.service\n\
                    Requires=metalos-snapshot-root.service\n\
                    ".to_string(),
                opts.normal_dir.join("metalos-switch-root.service.d/apply_host_config.conf") => "\
                    [Unit]\n\
                    After=metalos-apply-host-config.service\n\
                    Requires=metalos-apply-host-config.service\n\
                    ".to_string(),
                opts.environment_dir.join(ENVIRONMENT_FILENAME) => format!("\
                    HOST_CONFIG_URI=https://server:8000/config\n\
                    METALOS_BOOTS_DIR=/rootdisk/run/boot\n\
                    METALOS_CURRENT_BOOT_DIR=/rootdisk/run/boot/0:{}\n\
                    METALOS_IMAGES_DIR=/rootdisk/image\n\
                    METALOS_OS_PKG=somePackage\n\
                    ROOTDISK_DIR=/rootdisk\n\
                    ",
                    boot_id
                )
            },
        )
        .context("Failed to ensure tmpdir is setup correctly")?;

        Ok(())
    }

    #[test]
    fn test_generator_legacy() -> Result<()> {
        let (log, tmpdir, opts, _) =
            setup_generator_test("legacy").context("failed to setup test environment")?;

        let cmdline: MetalosCmdline = "\
            root=LABEL=unittest \
            rootflags=f1,f2,f3 \
            ro\
            "
        .parse()?;

        let boot_mode = generator_maybe_err(
            cmdline,
            log,
            opts.clone(),
            // We want to enforce that legacy doesn't use the disk searching functionality
            &MockErrDiskFinder {},
        )
        .context("failed to run generator")?;

        assert_eq!(boot_mode, BootMode::Legacy);

        compare_dir(
            &tmpdir,
            btreemap! {
                opts.normal_dir.join(ROOTDISK_MOUNT_SERVICE) => "\
                    [Unit]\n\
                    [Mount]\n\
                    What=LABEL=unittest\n\
                    Where=/rootdisk\n\
                    Options=f1,f2,f3,ro\n\
                ".to_string(),
                opts.environment_dir.join(ENVIRONMENT_FILENAME) => "".to_string(),
            },
        )
        .context("Failed to ensure tmpdir is setup correctly")?;

        Ok(())
    }

    #[test]
    fn test_metalos_reimage_boot_info() -> Result<()> {
        let (env, extra_deps, mount_unit) = metalos_reimage_boot_info(
            Root {
                root: Some("LABEL=unittest".to_string()),
                fstype: Some("testfs".to_string()),
                flags: Some(vec!["f1".to_string(), "f2".to_string(), "f3".to_string()]),
                ro: false,
                rw: true,
            },
            "test_config_uri".to_string(),
            "test_package:123".to_string(),
            "test_reimage_package:123".to_string(),
            &MockDiskFinder {
                disk: MockDisk {
                    disk: "/dev/unittest".into(),
                },
            },
        )
        .context("failed to get boot info")?;

        let boot_id = get_boot_id().context("failed to get boot id")?;

        assert_eq!(
            env,
            MetalosReimageEnvironment {
                metalos_common: MetalosEnvironment {
                    host_config_uri: "test_config_uri".to_string(),
                    rootdisk_dir: "/rootdisk".into(),
                    metalos_boots_dir: "/rootdisk/run/boot".into(),
                    metalos_current_boot_dir: format!("/rootdisk/run/boot/0:{}", boot_id).into(),
                    metalos_images_dir: "/rootdisk/image".into(),
                    os_package: "test_package:123".into(),
                },
                rootdisk_device: "/dev/unittest".into(),
                disk_image_package: "test_reimage_package:123".to_string(),
            }
        );

        assert_eq!(
            extra_deps,
            btreemap! {
                "metalos_boot".to_string() => ExtraDependency {
                    source: "metalos-switch-root.service".into(),
                    requires: "metalos-snapshot-root.service".into(),
                },
                "metalos_reimage_boot".to_string() => ExtraDependency {
                    source: "rootdisk.mount".into(),
                    requires: "metalos-image-root-disk.service".into(),
                },
                "apply_host_config".to_string() => ExtraDependency {
                    source: "metalos-switch-root.service".into(),
                    requires: "metalos-apply-host-config.service".into(),
                },
            }
        );

        assert_eq!(
            mount_unit,
            MountUnit {
                unit_section: UnitSection::default(),
                mount_section: MountSection {
                    what: "LABEL=unittest".into(),
                    where_: "/rootdisk".into(),
                    options: Some("f1,f2,f3,rw".to_string()),
                    type_: Some("testfs".to_string()),
                }
            }
        );

        Ok(())
    }

    #[test]
    fn test_metalos_existing_boot_info() -> Result<()> {
        let (env, extra_deps, mount_unit) = metalos_existing_boot_info(
            Root {
                root: Some("LABEL=unittest".to_string()),
                fstype: Some("testfs".to_string()),
                flags: Some(vec!["f1".to_string(), "f2".to_string(), "f3".to_string()]),
                ro: false,
                rw: true,
            },
            "test_config_uri".to_string(),
            "test_package:123".to_string(),
        )
        .context("failed to get boot info")?;

        let boot_id = get_boot_id().context("failed to get boot id")?;

        assert_eq!(
            env,
            MetalosEnvironment {
                host_config_uri: "test_config_uri".to_string(),
                rootdisk_dir: "/rootdisk".into(),
                metalos_boots_dir: "/rootdisk/run/boot".into(),
                metalos_current_boot_dir: format!("/rootdisk/run/boot/0:{}", boot_id).into(),
                metalos_images_dir: "/rootdisk/image".into(),
                os_package: "test_package:123".into(),
            }
        );

        assert_eq!(
            extra_deps,
            btreemap! {
                "metalos_boot".to_string() => ExtraDependency {
                    source: "metalos-switch-root.service".into(),
                    requires: "metalos-snapshot-root.service".into(),
                },
                "apply_host_config".to_string() => ExtraDependency {
                    source: "metalos-switch-root.service".into(),
                    requires: "metalos-apply-host-config.service".into(),
                },
            }
        );

        assert_eq!(
            mount_unit,
            MountUnit {
                unit_section: UnitSection::default(),
                mount_section: MountSection {
                    what: "LABEL=unittest".into(),
                    where_: "/rootdisk".into(),
                    options: Some("f1,f2,f3,rw".to_string()),
                    type_: Some("testfs".to_string()),
                }
            }
        );

        Ok(())
    }

    #[test]
    fn test_legacy_boot_info() -> Result<()> {
        let (env, extra_deps, mount_unit) = legacy_boot_info(Root {
            root: Some("LABEL=unittest".to_string()),
            fstype: Some("testfs".to_string()),
            flags: Some(vec!["f1".to_string(), "f2".to_string(), "f3".to_string()]),
            ro: false,
            rw: true,
        })
        .context("failed to get boot info")?;

        assert_eq!(env, LegacyEnvironment {});
        assert_eq!(extra_deps, ExtraDependencies::new());

        assert_eq!(
            mount_unit,
            MountUnit {
                unit_section: UnitSection::default(),
                mount_section: MountSection {
                    what: "LABEL=unittest".into(),
                    where_: "/rootdisk".into(),
                    options: Some("f1,f2,f3,rw".to_string()),
                    type_: Some("testfs".to_string()),
                }
            }
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
