/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use maplit::btreemap;
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use slog::{error, info, o, Drain, Logger};
use slog_glog_fmt::kv_categorizer::ErrorCategorizer;
use structopt::StructOpt;

use crate::kernel_cmdline::MetalosCmdline;
use crate::systemd::{self, PROVIDER_ROOT};

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
fn instantiate_template<U: AsRef<str>, I: AsRef<str>>(
    normal_dir: PathBuf,
    unit: U,
    instance: I,
) -> Result<String> {
    let instance_unit = systemd::template_unit_name(&unit, instance)?;
    let instance_file = normal_dir.join(&instance_unit);
    let template_src = PathBuf::from(PROVIDER_ROOT).join(unit.as_ref());
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
    fn add_kv(target: &mut String, key: &str, value: &str) {
        target.push_str(&format!("{}={}\n", key, value));
    }
    fn add_optional_kv<T: Into<String>>(target: &mut String, key: &str, value: Option<T>) {
        if let Some(value) = value {
            target.push_str(&format!("{}={}\n", key, value.into()));
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

struct UnitSection {
    before: Option<String>,
    after: Option<String>,
    requires: Option<String>,
}

impl Render for UnitSection {
    fn render(&self) -> String {
        let mut out = String::new();
        Self::add_header(&mut out, "Unit");
        Self::add_optional_kv(&mut out, "Before", self.before.as_ref());
        Self::add_optional_kv(&mut out, "After", self.after.as_ref());
        Self::add_optional_kv(&mut out, "Requires", self.requires.as_ref());
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
    if let Some(os_package) = cmdline.os_package() {
        info!(
            log,
            "instantiating metalos-fetch-image@{}.service", &os_package
        );
        let fetch_unit = instantiate_template(
            opts.normal_dir.clone(),
            "metalos-fetch-image@.service",
            os_package,
        )?;

        let switch_root_dropin = Dropin {
            target: "metalos-switch-root.service".into(),
            unit: Unit {
                unit: Some(UnitSection {
                    before: None,
                    after: Some(fetch_unit.clone()),
                    requires: Some(fetch_unit),
                }),
                body: Some(UnitBody::Service(ServiceSection {
                    environment: Some(btreemap! {
                        "OS_SUBVOL".to_string() => format!(
                            "var/lib/metalos/image/{}/volume",
                            systemd::escape(os_package)?
                        ),
                    }),
                })),
            },
        };
        switch_root_dropin
            .write_to_disk(log.clone(), &opts.normal_dir, "os_subvol.conf".as_ref())
            .context("Failed to write os_subvol.conf")?;
    }

    if let Some(host_config_uri) = cmdline.host_config_uri() {
        let uri_dropin = Dropin {
            target: "metalos-switch-root.service".to_string(),
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

    if let Some(root) = cmdline.root() {
        let root_src = blkid::evaluate_spec(&root.root)
            .with_context(|| format!("unable to understand root={}", root.root))?;

        let unit = Unit {
            unit: Some(UnitSection {
                before: Some("initrd-root-fs.target".to_string()),
                after: None,
                requires: None,
            }),
            body: Some(UnitBody::Mount(MountSection {
                what: (*root_src.to_string_lossy()).into(),
                where_: "/rootdisk".into(),
                options: root.flags,
                type_: root.fstype.map(|s| s.to_string()),
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

    if let Some(seed_device) = cmdline.seed_device() {
        info!(log, "Enabling seed device: {}", &seed_device);

        let seed_device_escaped = systemd::escape_path(&seed_device)?;
        let seedroot_service = format!("seedroot-device-add@{}.service", seed_device_escaped);

        instantiate_template(
            opts.normal_dir.clone(),
            "seedroot-device-add@.service",
            seed_device_escaped.clone(),
        )?;

        let seedroot_dropin = Dropin {
            target: "seedroot.service".into(),
            unit: Unit {
                unit: Some(UnitSection {
                    before: None,
                    after: Some(seedroot_service.clone()),
                    requires: Some(seedroot_service),
                }),
                body: None,
            },
        };
        seedroot_dropin
            .write_to_disk(log.clone(), &opts.normal_dir, "device-add.conf".as_ref())
            .context("Failed to write device-add.conf")?;
    }

    Ok(())
}

pub fn generator(log: Logger, opts: Opts) -> Result<()> {
    // metalos-generator has an additional logging drain setup that is not as
    // pretty looking as other slog drain formats, but is usable with /dev/kmsg.
    // Otherwise, the regular drain that logs to stderr silently disappears when
    // systemd runs the generator.
    let kmsg = fs::OpenOptions::new()
        .write(true)
        .open("/dev/kmsg")
        .context("failed to open /dev/kmsg for logging")?;
    let kmsg = BufWriter::new(kmsg);

    let decorator = slog_term::PlainDecorator::new(kmsg);
    let drain = slog_glog_fmt::GlogFormat::new(decorator, ErrorCategorizer).fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    let log = slog::Logger::root(slog::Duplicate::new(log, drain).fuse(), o!());

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
    use crate::systemd::PROVIDER_ROOT;
    use std::path::Path;
    use std::time::SystemTime;

    #[test]
    fn instantiate_example_template() -> Result<()> {
        let ts = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        let tmpdir = std::env::temp_dir().join(format!("instantiate_template_{:?}", ts));
        std::fs::create_dir(&tmpdir)?;
        instantiate_template(tmpdir.clone(), "hello@.service", "world")?;
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
            metalos.package_format_uri=\"someURI\" \
            metalos.seed_device=\"seedDevice\" \
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
        assert_eq!(
            tmpdir
                .join("seedroot-device-add@seedDevice.service")
                .read_link()?,
            Path::new(PROVIDER_ROOT).join("seedroot-device-add@.service"),
        );

        let file = tmpdir.join("metalos-switch-root.service.d/host_config_uri.conf");
        assert!(file.exists());
        let content = std::fs::read_to_string(file.clone())
            .context(format!("Can't read file {}", file.display()))?;
        assert_eq!(
            content,
            "[Service]\nEnvironment=HOST_CONFIG_URI=https://server:8000/v1/host/host001.01.abc0.domain.com\n"
        );

        let file = tmpdir.join("metalos-switch-root.service.d/os_subvol.conf");
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

        let file = tmpdir.join("seedroot.service.d/device-add.conf");
        assert!(file.exists());
        let content = std::fs::read_to_string(file.clone())
            .context(format!("Can't read file {}", file.display()))?;
        assert_eq!(
            content,
            "[Unit]\n\
            After=seedroot-device-add@seedDevice.service\n\
            Requires=seedroot-device-add@seedDevice.service\n"
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
            metalos.package_format_uri=\"someURI\" \
            metalos.seed_device=\"seedDevice\" \
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
