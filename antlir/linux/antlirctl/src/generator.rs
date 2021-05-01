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

// This helper intentionally not implemented in systemd.rs. The generator has to
// operate on files since systemd might not be ready while the generator is
// running. Post-generator interactions with systemd should happen over dbus,
// not filesystem mangling.
fn instantiate_and_enable_template<U: AsRef<str>, I: AsRef<str>, W: AsRef<str>>(
    normal_dir: PathBuf,
    unit: U,
    instance: I,
    wanted_by: W,
) -> Result<()> {
    let instance_unit = systemd::template_unit_name(&unit, instance)?;
    let instance_file = normal_dir.join(&instance_unit);
    let template_src = PathBuf::from(PROVIDER_ROOT).join(unit.as_ref());
    symlink(&template_src, &instance_file).with_context(|| {
        format!(
            "failed to symlink {:?} -> {:?}",
            instance_file, template_src
        )
    })?;
    let wants_dir = normal_dir.join(format!("{}.wants", wanted_by.as_ref()));
    fs::create_dir_all(&wants_dir).with_context(|| format!("failed to create {:?}", wants_dir))?;
    symlink(&instance_file, wants_dir.join(&instance_unit)).with_context(|| {
        format!(
            "failed to symlink {:?} -> {:?}",
            wants_dir.join(&instance_unit),
            instance_file,
        )
    })?;
    Ok(())
}

fn generator_maybe_err(log: Logger, opts: Opts) -> Result<()> {
    let cmdline =
        AntlirCmdline::from_kernel().context("invalid kernel cmdline options for Antlir")?;

    if let Some(control_os_uri) = cmdline.control_os_uri() {
        info!(
            log,
            "Instantiating antlir-fetch-image@.service";
        );
        instantiate_and_enable_template(
            opts.normal_dir.clone(),
            "antlir-fetch-image@.service",
            control_os_uri,
            "initrd.target",
        )?;
    }

    if let Some(root) = cmdline.root() {
        let sysroot_unit_path = opts.normal_dir.join("sysroot.mount");
        let mut unit = fs::File::create(&sysroot_unit_path)
            .with_context(|| format!("failed to open {:?} for writing", sysroot_unit_path))?;
        let root_src = crate::mount::source_to_device_path(&root.root)
            .with_context(|| format!("unable to understand root={}", root.root))?;
        write!(
            unit,
            "[Unit]\nBefore=initrd-root-fs.target\n[Mount]\nWhat={}\nWhere=/sysroot\nOptions={}\nType={}\n",
            root_src.to_string_lossy(),
            root.flags.unwrap_or_else(|| "".to_string()),
            root.fstype.unwrap_or(""),
        )
        .context("failed to write sysroot.mount")?;
        let requires_dir = opts.normal_dir.join("initrd-root-fs.target.requires");
        fs::create_dir(&requires_dir)?;
        symlink(&sysroot_unit_path, requires_dir.join("sysroot.mount"))?;
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
        instantiate_and_enable_template(
            tmpdir.clone(),
            "hello@.service",
            "world",
            "multi-user.target",
        )?;
        assert_eq!(
            tmpdir.join("hello@world.service").read_link()?,
            Path::new(PROVIDER_ROOT).join("hello@.service"),
        );
        assert_eq!(
            tmpdir
                .join("multi-user.target.wants/hello@world.service")
                .read_link()?,
            tmpdir.join("hello@world.service"),
        );
        std::fs::remove_dir_all(&tmpdir)?;
        Ok(())
    }
}
