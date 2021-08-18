/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::io::{BufWriter, Write};
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use anyhow::{Context, Result};
use slog::{error, info, o, Drain, Logger};
use slog_glog_fmt::kv_categorizer::ErrorCategorizer;
use structopt::StructOpt;

use crate::kernel_cmdline::AntlirCmdline;
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

fn generator_maybe_err(log: Logger, opts: Opts) -> Result<()> {
    let cmdline =
        AntlirCmdline::from_kernel().context("invalid kernel cmdline options for Antlir")?;

    if let Some(os_package) = cmdline.os_package() {
        info!(
            log,
            "instantiating antlir-fetch-image@{}.service", &os_package
        );
        let fetch_unit = instantiate_template(
            opts.normal_dir.clone(),
            "antlir-fetch-image@.service",
            os_package,
        )?;
        fs::create_dir_all(&opts.normal_dir.join("antlir-switch-root.service.d"))
            .context("failed to create .d/ for antlir-switch-root.service")?;
        let mut subvol_conf = fs::File::create(
            opts.normal_dir
                .join("antlir-switch-root.service.d")
                .join("os_subvol.conf"),
        )
        .context("failed to create drop-in for antlir-switch-root.service")?;
        info!(
            log,
            "Writing drop-in to switch-root into subvol for {}", &os_package,
        );
        info!(
            log,
            "antlir-switch-root.service will wait for {}", &fetch_unit,
        );
        write!(
            subvol_conf,
            "[Unit]\nAfter={}\nRequires={}\n[Service]\nEnvironment=OS_SUBVOL=var/lib/antlir/image/{}/volume",
            fetch_unit,
            fetch_unit,
            systemd::escape(os_package)?
        )?;
    }

    if let Some(root) = cmdline.root() {
        let rootdisk_unit_path = opts.normal_dir.join("rootdisk.mount");
        let mut unit = fs::File::create(&rootdisk_unit_path)
            .with_context(|| format!("failed to open {:?} for writing", rootdisk_unit_path))?;
        let root_src = crate::mount::source_to_device_path(&root.root)
            .with_context(|| format!("unable to understand root={}", root.root))?;
        write!(
            unit,
            "[Unit]\nBefore=initrd-root-fs.target\n[Mount]\nWhat={}\nWhere=/rootdisk\nOptions={}\nType={}\n",
            root_src.to_string_lossy(),
            root.flags.unwrap_or_else(|| "".to_string()),
            root.fstype.unwrap_or(""),
        )
        .context("failed to write rootdisk.mount")?;
        let requires_dir = opts.normal_dir.join("initrd-root-fs.target.requires");
        fs::create_dir(&requires_dir)?;
        symlink(&rootdisk_unit_path, requires_dir.join("rootdisk.mount"))?;
    }

    if let Some(seed_device) = cmdline.seed_device() {
        info!(log, "Enabling seed device: {}", &seed_device);

        let seed_device_escaped = systemd::escape_path(&seed_device)?;

        instantiate_template(
            opts.normal_dir.clone(),
            "seedroot-device-add@.service",
            seed_device_escaped.clone(),
        )?;

        let seedroot_dropin_dir = opts.normal_dir.join("seedroot.service.d");
        fs::create_dir(&seedroot_dropin_dir)?;
        let seedroot_dropin_path = seedroot_dropin_dir.join("device-add.conf");
        let mut unit = fs::File::create(&seedroot_dropin_path)
            .with_context(|| format!("failed to open {:?} for writing", seedroot_dropin_path))?;
        write!(
            unit,
            "[Unit]\nRequires=seedroot-device-add@{}.service\nAfter=seedroot-device-add@{}.service",
            seed_device_escaped, seed_device_escaped,
        )
        .context("failed to write seedroot.service dropin")?;
    }

    Ok(())
}

pub fn generator(log: Logger, opts: Opts) -> Result<()> {
    // antlir-generator has an additional logging drain setup that is not as
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

    info!(log, "antlir-generator starting");

    let sublog = log.new(o!());

    match generator_maybe_err(sublog, opts) {
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
}
