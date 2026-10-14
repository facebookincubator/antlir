/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Write as _;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;

use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

use crate::BuildAppliance;
use crate::PackageFormat;
use crate::run_cmd;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deb {
    build_appliance: BuildAppliance,
    #[serde(rename = "deb_name")]
    name: String,
    version: String,
    arch: String,
    maintainer: Option<String>,
    section: Option<String>,
    priority: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    #[serde(default)]
    pre_depends: Vec<String>,
    #[serde(default)]
    depends: Vec<String>,
    #[serde(default)]
    recommends: Vec<String>,
    #[serde(default)]
    suggests: Vec<String>,
    #[serde(default)]
    provides: Vec<String>,
    #[serde(default)]
    conflicts: Vec<String>,
    #[serde(default)]
    breaks: Vec<String>,
    #[serde(default)]
    replaces: Vec<String>,
    preinst: Option<String>,
    postinst: Option<String>,
    prerm: Option<String>,
    postrm: Option<String>,
}

impl Deb {
    fn control(&self) -> Result<String> {
        let mut control = String::new();
        writeln!(control, "Package: {}", self.name)?;
        writeln!(control, "Version: {}", self.version)?;
        writeln!(control, "Architecture: {}", self.arch)?;
        for (field, value) in [
            ("Maintainer", &self.maintainer),
            ("Section", &self.section),
            ("Priority", &self.priority),
        ] {
            if let Some(value) = value {
                writeln!(control, "{field}: {value}")?;
            }
        }
        for (field, relations) in [
            ("Pre-Depends", &self.pre_depends),
            ("Depends", &self.depends),
            ("Recommends", &self.recommends),
            ("Suggests", &self.suggests),
            ("Provides", &self.provides),
            ("Conflicts", &self.conflicts),
            ("Breaks", &self.breaks),
            ("Replaces", &self.replaces),
        ] {
            if !relations.is_empty() {
                writeln!(control, "{field}: {}", relations.join(", "))?;
            }
        }
        writeln!(
            control,
            "Description: {}",
            self.summary.as_deref().unwrap_or(&self.name)
        )?;
        // Extended description lines are indented by one space, and blank
        // lines are spelled " ."
        if let Some(description) = &self.description {
            for line in description.trim_end().lines() {
                match line.trim().is_empty() {
                    true => control.push_str(" .\n"),
                    false => writeln!(control, " {line}")?,
                }
            }
        }
        Ok(control)
    }

    fn maintainer_scripts(&self) -> impl Iterator<Item = (&'static str, &str)> {
        [
            ("preinst", &self.preinst),
            ("postinst", &self.postinst),
            ("prerm", &self.prerm),
            ("postrm", &self.postrm),
        ]
        .into_iter()
        .filter_map(|(name, script)| script.as_deref().map(|s| (name, s)))
    }
}

impl PackageFormat for Deb {
    fn build(&self, out: &Path, layer: &Path) -> Result<()> {
        let tmp = tempfile::tempdir().context("while creating temp dir for deb build")?;
        let control_dir = tmp.path().join("pkg/DEBIAN");
        std::fs::create_dir_all(&control_dir).context("while creating DEBIAN dir")?;
        std::fs::set_permissions(&control_dir, Permissions::from_mode(0o755))
            .context("while setting DEBIAN dir permissions")?;
        std::fs::write(control_dir.join("control"), self.control()?)
            .context("while writing control file")?;

        for (name, script) in self.maintainer_scripts() {
            let path = control_dir.join(name);
            // Like rpm scriptlets, scripts without a shebang are treated as
            // a /bin/sh body
            let contents = match script.starts_with("#!") {
                true => script.to_owned(),
                false => format!("#!/bin/sh\nset -e\n{script}\n"),
            };
            std::fs::write(&path, contents).with_context(|| format!("while writing {name}"))?;
            std::fs::set_permissions(&path, Permissions::from_mode(0o755))
                .with_context(|| format!("while setting {name} permissions"))?;
        }

        let mut isol_context = IsolationContext::builder(self.build_appliance.path());
        isol_context
            .ephemeral(false)
            .readonly()
            .hostname("antlir2")
            .working_directory(Path::new("/"))
            .inputs((Path::new("/__antlir2__/root"), layer))
            .outputs((Path::new("/__antlir2__/out.deb"), out))
            .outputs((Path::new("/tmp"), tmp.path()))
            .tmpfs(Path::new("/dev"))
            .inputs(Path::new("/dev/null"));
        let isol_context = isol_context.build();

        // The build appliance has no shell or cp, so stage the layer contents
        // next to DEBIAN/ with a tar round trip (which preserves ownership)
        run_cmd(
            unshare(isol_context.clone())?
                .command("tar")?
                .args(["--numeric-owner", "-C", "/__antlir2__/root"])
                .args(["-cf", "/tmp/layer.tar", "."])
                .stdout(Stdio::piped()),
        )
        .context("failed to archive layer")?;
        run_cmd(
            unshare(isol_context.clone())?
                .command("tar")?
                .args(["--numeric-owner", "--same-owner", "--same-permissions"])
                .args(["-C", "/tmp/pkg", "-xf", "/tmp/layer.tar"])
                .stdout(Stdio::piped()),
        )
        .context("failed to stage layer")?;

        // Ubuntu's dpkg defaults to zstd, so explicitly use xz to produce the
        // same (and more widely readable) archives regardless of the build
        // appliance distro.
        run_cmd(
            unshare(isol_context)?
                .command("dpkg-deb")?
                .args(["-Zxz", "--build", "/tmp/pkg", "/__antlir2__/out.deb"])
                .stdout(Stdio::piped()),
        )
        .context("failed to build deb")?;

        // fail loudly if there was a permissions error removing the temporary
        // directory, otherwise a later buck build will fail with permissions
        // errors - spooky action at a distance
        tmp.close().context("while cleaning up tmpdir")?;

        Ok(())
    }
}
