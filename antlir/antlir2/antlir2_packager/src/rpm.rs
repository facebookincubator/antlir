/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

use antlir2_isolate::isolate;
use antlir2_isolate::IsolationContext;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use itertools::Itertools;
use serde::Deserialize;
use tempfile::NamedTempFile;

use crate::run_cmd;
use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
pub struct Rpm {
    build_appliance: PathBuf,
    layer: PathBuf,
    #[serde(rename = "rpm_name")]
    name: String,
    epoch: i32,
    version: String,
    release: String,
    arch: String,
    license: String,
    summary: Option<String>,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    recommends: Vec<String>,
    #[serde(default)]
    provides: Vec<String>,
    #[serde(default)]
    supplements: Vec<String>,
    #[serde(default)]
    conflicts: Vec<String>,
    description: Option<String>,
    post_install_script: Option<String>,
}

impl PackageFormat for Rpm {
    fn build(&self, out: &Path) -> Result<()> {
        let layer_abspath = self
            .layer
            .canonicalize()
            .with_context(|| format!("failed to build absolute path to layer {:?}", self.layer))?;

        let requires = self
            .requires
            .iter()
            .map(|r| format!("Requires: {r}"))
            .join("\n");

        let recommends = self
            .recommends
            .iter()
            .map(|r| format!("Recommends: {r}"))
            .join("\n");

        let provides = self
            .provides
            .iter()
            .map(|p| format!("Provides: {p}"))
            .join("\n");

        let supplements = self
            .supplements
            .iter()
            .map(|p| format!("Supplements: {p}"))
            .join("\n");

        let conflicts = self
            .conflicts
            .iter()
            .map(|p| format!("Conflicts: {p}"))
            .join("\n");

        let Self {
            name,
            epoch,
            version,
            release,
            arch,
            license,
            ..
        } = self;

        let mut spec = format!(
            r#"Name: {name}
Epoch: {epoch}
Version: {version}
Release: {release}
BuildArch: {arch}

Summary: {summary}
License: {license}

{requires}
{recommends}
{provides}
{supplements}
{conflicts}

%description
{description}

{post_install_script}

"#,
            summary = self.summary.as_deref().unwrap_or(name.as_str()),
            requires = requires,
            recommends = recommends,
            provides = provides,
            supplements = supplements,
            conflicts = conflicts,
            description = self.description.as_deref().unwrap_or_default(),
            post_install_script = self
                .post_install_script
                .as_ref()
                .map(|s| format!("%post\n{s}\n"))
                .unwrap_or_default(),
        );
        if std::fs::read_dir(&self.layer)
            .context("failed to list layer contents")?
            .count()
            != 0
        {
            spec.push_str("%install\n");
            writeln!(
                spec,
                "cp -rp \"{layer}\"/* %{{buildroot}}/",
                layer = layer_abspath.display()
            )?;
            spec.push_str("%files\n");
            for entry in walkdir::WalkDir::new(&self.layer) {
                let entry = entry.context("while walking layer")?;
                if entry.file_type().is_dir() {
                    continue;
                }
                let relpath = Path::new("/").join(
                    entry
                        .path()
                        .strip_prefix(&self.layer)
                        .expect("must be under layer"),
                );
                if relpath == Path::new("/") {
                    continue;
                }
                spec.push_str(relpath.to_str().expect("our paths are always valid utf8"));
                spec.push('\n');
            }
        } else {
            spec.push_str("%files\n");
        }
        let mut rpm_spec_file =
            NamedTempFile::new().context("failed to create tempfile for rpm spec")?;
        rpm_spec_file
            .write(spec.as_bytes())
            .context("while writing rpm spec file")?;

        let output_dir = tempfile::tempdir().context("while creating temp dir for rpm output")?;

        // create the arch-specific output dir explicitly so that it'll be
        // owned by the build user on the host, not root
        std::fs::create_dir(output_dir.path().join(&self.arch))
            .context("while creating output dir")?;

        let isol_context = IsolationContext::builder(&self.build_appliance)
            .inputs([rpm_spec_file.path(), self.layer.as_path()])
            .outputs([output_dir.path()])
            .working_directory(std::env::current_dir().context("while getting cwd")?)
            .build();

        run_cmd(
            isolate(isol_context)?
                .command("/bin/rpmbuild")?
                .arg("-bb")
                .arg("--define")
                .arg(format!("_rpmdir {}", output_dir.path().display()))
                .arg(rpm_spec_file.path())
                .stdout(Stdio::piped()),
        )
        .context("Failed to build rpm")?;

        let outputs: Vec<_> = output_dir
            .path()
            .join(&self.arch)
            .read_dir()
            .context("while reading rpm output dir")?
            .filter_map(Result::ok)
            .collect();

        ensure!(
            outputs.len() == 1,
            "expected exactly one output rpm file, got: {outputs:?}"
        );

        std::fs::copy(outputs[0].path(), out).context("while moving output to correct location")?;

        // fail loudly if there was a permissions error removing the
        // temporary output directory, otherwise a later buck build will
        // fail with permissions errors - spooky action at a distance
        output_dir
            .close()
            .context("while cleaning up output tmpdir")?;

        Ok(())
    }
}
