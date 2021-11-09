/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use maplit::btreemap;
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use slog::{error, info, o, Logger};
use structopt::StructOpt;

use crate::kernel_cmdline::MetalosCmdline;
use crate::mount::evaluate_device_spec;
use systemd::{self, UnitName, PROVIDER_ROOT};

#[derive(StructOpt)]
pub struct Opts {
    normal_dir: PathBuf,
    #[allow(unused)]
    early_dir: PathBuf,
    #[allow(unused)]
    late_dir: PathBuf,
}

// This unit file helper intentionally not implemented in systemd.rs. The
// generator has to operate on files since systemd might not be ready while the
// generator is running. Post-generator interactions with systemd should happen
// over dbus, not filesystem mangling. See the systemd generator docs for more
// details about the limitations imposed on generators.
// https://www.freedesktop.org/software/systemd/man/systemd.generator.html#Notes%20about%20writing%20generators
fn instantiate_template(
    normal_dir: PathBuf,
    unit: impl AsRef<str>,
    instance: impl AsRef<str>,
    suffix: impl AsRef<str>,
) -> Result<UnitName> {
    let instance_unit = systemd::template_unit_name(&unit, instance, &suffix);
    let instance_file = normal_dir.join(&instance_unit);
    let template_src =
        PathBuf::from(PROVIDER_ROOT).join(format!("{}@.{}", unit.as_ref(), suffix.as_ref()));
    symlink(&template_src, &instance_file).with_context(|| {
        format!(
            "failed to symlink {:?} -> {:?}",
            instance_file, template_src
        )
    })?;
    Ok(instance_unit)
}

trait Render {
    fn render(&self) -> String;

    fn add_header(target: &mut String, name: &str) {
        target.push_str(&format!("[{}]\n", name));
    }
    fn add_kv<T: std::fmt::Display>(target: &mut String, key: &str, value: T) {
        target.push_str(&format!("{}={}\n", key, value));
    }
    fn add_optional_kv<T: std::fmt::Display>(target: &mut String, key: &str, value: Option<T>) {
        if let Some(value) = value {
            target.push_str(&format!("{}={}\n", key, value));
        }
    }
    fn add_optional_renderable<T: Render>(target: &mut String, thing: Option<&T>) {
        if let Some(thing) = thing {
            target.push_str(&thing.render());
        }
    }
}

struct ServiceSection {
    environment: Option<BTreeMap<String, String>>,
}

impl Render for ServiceSection {
    fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("[Service]\n");
        if let Some(env) = &self.environment {
            for (k, v) in env.iter() {
                Self::add_kv(&mut out, "Environment", &format!("{}={}", k, v));
            }
        }
        out
    }
}

#[derive(Default)]
struct UnitSection {
    before: Option<UnitName>,
    after: Option<UnitName>,
    requires: Option<UnitName>,
    timeout: Option<Duration>,
}

impl Render for UnitSection {
    fn render(&self) -> String {
        let mut out = String::new();
        Self::add_header(&mut out, "Unit");
        Self::add_optional_kv(&mut out, "Before", self.before.as_ref());
        Self::add_optional_kv(&mut out, "After", self.after.as_ref());
        Self::add_optional_kv(&mut out, "Requires", self.requires.as_ref());
        if let Some(timeout) = &self.timeout {
            Self::add_kv(
                &mut out,
                "JobRunningTimeoutSec",
                &timeout.as_secs().to_string(),
            );
        }
        out
    }
}

struct MountSection {
    what: PathBuf,
    where_: PathBuf,
    options: Option<String>,
    type_: Option<String>,
}

impl Render for MountSection {
    fn render(&self) -> String {
        let mut out = String::new();
        Self::add_header(&mut out, "Mount");
        Self::add_kv(&mut out, "What", &self.what.to_string_lossy());
        Self::add_kv(&mut out, "Where", &self.where_.to_string_lossy());
        Self::add_kv(
            &mut out,
            "Options",
            match &self.options {
                Some(opts) => opts,
                None => "",
            },
        );
        Self::add_optional_kv(&mut out, "Type", self.type_.as_ref());
        out
    }
}

enum UnitBody {
    Service(ServiceSection),
    Mount(MountSection),
}

impl Render for UnitBody {
    fn render(&self) -> String {
        match self {
            Self::Service(s) => s.render(),
            Self::Mount(s) => s.render(),
        }
    }
}

struct Unit {
    unit: Option<UnitSection>,
    body: Option<UnitBody>,
}

impl Unit {
    fn write_to_disk(
        &self,
        log: Logger,
        base_dir: &Path,
        unit_name: &str,
        filename: &Path,
    ) -> Result<()> {
        let unit_file_path = base_dir.join(filename);
        let mut file = std::fs::File::create(&unit_file_path).context(format!(
            "Failed to create unit file {:?} in {:?}",
            filename, base_dir
        ))?;

        if let Some(unit) = &self.unit {
            if let Some(before) = &unit.before {
                let requires_dir = base_dir.join(PathBuf::from(format!("{}.requires", before)));
                std::fs::create_dir(&requires_dir)
                    .context(format!("Failed to create requires dir for {}", before))?;
                symlink(&unit_file_path, requires_dir.join(unit_name))?;
            }
        }

        let content = self.render();
        info!(log, "Writing to {:?}:\n{}", unit_file_path, content);
        write!(file, "{}", content).context("Failed to write rendered content to file")?;
        Ok(())
    }
}

impl Render for Unit {
    fn render(&self) -> String {
        let mut out = String::new();
        Self::add_optional_renderable(&mut out, self.unit.as_ref());
        Self::add_optional_renderable(&mut out, self.body.as_ref());
        out
    }
}

struct Dropin {
    target: String,
    unit: Unit,
}

impl Dropin {
    fn write_to_disk(&self, log: Logger, base_dir: &Path, filename: &Path) -> Result<()> {
        let dropin_path: PathBuf = format!("{}.d", self.target).into();
        let service_dir: PathBuf = base_dir.join(&dropin_path);
        std::fs::create_dir_all(&service_dir)
            .context(format!("failed to create .d/ for {}", self.target))?;

        info!(log, "Writing drop-in {:?} for {}", filename, self.target);

        if let Some(unit_section) = &self.unit.unit {
            if let Some(after) = &unit_section.after {
                info!(log, "{} will wait for {}", self.target, after);
            }
            if let Some(before) = &unit_section.before {
                info!(log, "{} will apply before {}", self.target, before);
            }
        }

        self.unit
            .write_to_disk(log, base_dir, &self.target, &service_dir.join(filename))?;
        Ok(())
    }
}

fn generator_maybe_err(cmdline: MetalosCmdline, log: Logger, opts: Opts) -> Result<()> {
    if let Some(os_package) = &cmdline.os_package {
        info!(
            log,
            "instantiating metalos-fetch-image@{}.service", &os_package
        );
        let fetch_unit = instantiate_template(
            opts.normal_dir.clone(),
            "metalos-fetch-image",
            os_package,
            "service",
        )?;

        let snapshot_root_dropin = Dropin {
            target: "metalos-snapshot-root.service".into(),
            unit: Unit {
                unit: Some(UnitSection {
                    after: Some(fetch_unit.clone()),
                    requires: Some(fetch_unit),
                    ..Default::default()
                }),
                body: Some(UnitBody::Service(ServiceSection {
                    environment: Some(btreemap! {
                        "OS_SUBVOL".to_string() => format!(
                            "var/lib/metalos/image/{}/volume",
                            systemd::escape(os_package)
                        ),
                    }),
                })),
            },
        };
        snapshot_root_dropin
            .write_to_disk(log.clone(), &opts.normal_dir, "os_subvol.conf".as_ref())
            .context("Failed to write os_subvol.conf")?;
    }

    if let Some(host_config_uri) = &cmdline.host_config_uri {
        let uri_dropin = Dropin {
            target: "metalos-apply-host-config.service".to_string(),
            unit: Unit {
                unit: None,
                body: Some(UnitBody::Service(ServiceSection {
                    environment: Some(btreemap! {
                        "HOST_CONFIG_URI".to_string() => host_config_uri.to_string()
                    }),
                })),
            },
        };
        uri_dropin
            .write_to_disk(
                log.clone(),
                &opts.normal_dir,
                "host_config_uri.conf".as_ref(),
            )
            .context("Failed to write host_config_uri.conf")?;
    }

    if let Some(root) = &cmdline.root.root {
        // if we don't have blkid available, we have to hope that the given
        // root= parameter is specified enough (aka, is an absolute device path)
        let root_src = evaluate_device_spec(root)
            .with_context(|| format!("unable to understand root={}", root))?;

        let unit = Unit {
            unit: Some(UnitSection {
                before: Some("initrd-root-fs.target".into()),
                ..Default::default()
            }),
            body: Some(UnitBody::Mount(MountSection {
                what: (*root_src.to_string_lossy()).into(),
                where_: "/rootdisk".into(),
                options: cmdline.root.join_flags(),
                type_: cmdline.root.fstype,
            })),
        };

        unit.write_to_disk(
            log.clone(),
            &opts.normal_dir,
            "rootdisk.mount",
            "rootdisk.mount".as_ref(),
        )
        .context("Failed to write rootdisk.mount")?;
    }

    Ok(())
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

    match generator_maybe_err(cmdline, sublog, opts) {
        Ok(()) => Ok(()),
        Err(e) => {
            error!(log, "{}", e.to_string());
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::SystemTime;
    use systemd::PROVIDER_ROOT;

    #[test]
    fn instantiate_example_template() -> Result<()> {
        let ts = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        let tmpdir = std::env::temp_dir().join(format!("instantiate_template_{:?}", ts));
        std::fs::create_dir(&tmpdir)?;
        instantiate_template(tmpdir.clone(), "hello", "world", "service")?;
        assert_eq!(
            tmpdir.join("hello@world.service").read_link()?,
            Path::new(PROVIDER_ROOT).join("hello@.service"),
        );
        std::fs::remove_dir_all(&tmpdir)?;
        Ok(())
    }

    #[test]
    fn test_generator() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
        let ts = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        let tmpdir = std::env::temp_dir().join(format!("test_generator{:?}", ts));
        std::fs::create_dir(&tmpdir)?;
        let opts = Opts {
            normal_dir: tmpdir.clone(),
            early_dir: tmpdir.clone(),
            late_dir: tmpdir.clone(),
        };
        // here we pass rootfstype=btrfs
        let cmdline: MetalosCmdline =
            "metalos.host-config-uri=\"https://server:8000/v1/host/host001.01.abc0.domain.com\" \
            metalos.os_package=\"somePackage\" \
            metalos.package_format_uri=\"https://unittest_server/{package}\" \
            rootfstype=btrfs \
            root=/dev/somedisk"
                .parse()?;

        generator_maybe_err(cmdline, log, opts)?;

        assert_eq!(
            tmpdir
                .join("metalos-fetch-image@somePackage.service")
                .read_link()?,
            Path::new(PROVIDER_ROOT).join("metalos-fetch-image@.service"),
        );

        let file = tmpdir.join("metalos-apply-host-config.service.d/host_config_uri.conf");
        assert!(file.exists());
        let content = std::fs::read_to_string(file.clone())
            .context(format!("Can't read file {}", file.display()))?;
        assert_eq!(
            content,
            "[Service]\nEnvironment=HOST_CONFIG_URI=https://server:8000/v1/host/host001.01.abc0.domain.com\n"
        );

        let file = tmpdir.join("metalos-snapshot-root.service.d/os_subvol.conf");
        assert!(file.exists());
        let content = std::fs::read_to_string(file.clone())
            .context(format!("Can't read file {}", file.display()))?;
        assert_eq!(
            content,
            "[Unit]\n\
            After=metalos-fetch-image@somePackage.service\n\
            Requires=metalos-fetch-image@somePackage.service\n\
            [Service]\n\
            Environment=OS_SUBVOL=var/lib/metalos/image/somePackage/volume\n"
        );

        let file = tmpdir.join("rootdisk.mount");
        assert!(file.exists());
        let content = std::fs::read_to_string(file.clone())
            .context(format!("Can't read file {}", file.display()))?;
        assert_eq!(
            content,
            "[Unit]\n\
            Before=initrd-root-fs.target\n\
            [Mount]\n\
            What=/dev/somedisk\n\
            Where=/rootdisk\n\
            Options=\n\
            Type=btrfs\n"
        );

        std::fs::remove_dir_all(&tmpdir)?;

        Ok(())
    }

    #[test]
    fn test_generator_no_rootfstype() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
        let ts = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        let tmpdir = std::env::temp_dir().join(format!("test_generator_nofstype{:?}", ts));
        std::fs::create_dir(&tmpdir)?;
        let opts = Opts {
            normal_dir: tmpdir.clone(),
            early_dir: tmpdir.clone(),
            late_dir: tmpdir.clone(),
        };
        // here we do not pass rootfstype=btrfs
        let cmdline: MetalosCmdline =
            "metalos.host-config-uri=\"https://server:8000/v1/host/host001.01.abc0.domain.com\" \
            metalos.os_package=\"somePackage\" \
            metalos.package_format_uri=\"https://unittest_server/{package}\" \
            root=/dev/somedisk"
                .parse()?;

        generator_maybe_err(cmdline, log, opts)?;

        let file = tmpdir.join("rootdisk.mount");
        assert!(file.exists());
        let content = std::fs::read_to_string(file.clone())
            .context(format!("Can't read file {}", file.display()))?;
        assert_eq!(
            content,
            "[Unit]\n\
            Before=initrd-root-fs.target\n\
            [Mount]\n\
            What=/dev/somedisk\n\
            Where=/rootdisk\n\
            Options=\n"
        );

        std::fs::remove_dir_all(&tmpdir)?;
        Ok(())
    }
}
