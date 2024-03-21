/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Write as _;
use std::fs::File;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

use antlir2_isolate::unshare;
use antlir2_isolate::IsolationContext;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use chrono::prelude::*;
use itertools::Itertools;
use libcap::FileExt as _;
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
    version: Option<String>,
    release: Option<String>,
    arch: String,
    license: String,
    summary: Option<String>,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    requires_post: Vec<String>,
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
    sign_with_private_key: Option<PathBuf>,
    sign_digest_algo: Option<String>,
    changelog: Option<String>,
    #[serde(default)]
    extra_files: Vec<String>,
    #[serde(default)]
    python_bytecompile: bool,
}

impl PackageFormat for Rpm {
    fn build(&self, out: &Path) -> Result<()> {
        let requires = self
            .requires
            .iter()
            .map(|r| format!("Requires: {r}"))
            .join("\n");

        let requires_post = self
            .requires_post
            .iter()
            .map(|r| format!("Requires(post): {r}"))
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

        let localtime: DateTime<Local> = Local::now();
        let mut spec = format!(
            r#"Name: {name}
Epoch: {epoch}
Version: {version}
Release: {release}
BuildArch: {arch}

Summary: {summary}
License: {license}

{requires}
{requires_post}
{recommends}
{provides}
{supplements}
{conflicts}

%description
{description}

{changelog}

{post_install_script}
"#,
            summary = self.summary.as_deref().unwrap_or(name.as_str()),
            version = version
                .as_deref()
                .unwrap_or(&localtime.format("%Y%m%d").to_string()),
            release = release
                .as_deref()
                .unwrap_or(&localtime.format("%H%M%S").to_string()),
            requires = requires,
            requires_post = requires_post,
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
            changelog = self
                .changelog
                .as_ref()
                .map(|c| format!("%changelog\n{c}\n"))
                .unwrap_or_default(),
        );

        if !self.python_bytecompile {
            spec.push_str("%define __brp_python_bytecompile %{nil}\n");
        }

        if std::fs::read_dir(&self.layer)
            .context("failed to list layer contents")?
            .count()
            != 0
        {
            spec.push_str("%install\n");
            writeln!(spec, "cp -rp \"/__antlir2__/root\"/* %{{buildroot}}/",)?;
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
                if let Some(caps) = File::open(entry.path()).and_then(|f| f.get_capabilities())? {
                    let caps = caps.to_text()?;
                    spec.push_str("%caps(");
                    spec.push_str(&caps);
                    spec.push_str(") ");
                }

                spec.push_str(relpath.to_str().expect("our paths are always valid utf8"));
                spec.push('\n');
            }
        } else {
            spec.push_str("%files\n");
        }
        for extra_file in &self.extra_files {
            spec.push_str(extra_file);
            spec.push('\n');
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

        let mut isol_context = IsolationContext::builder(&self.build_appliance);
        isol_context
            .ephemeral(false)
            .readonly()
            .hostname("antlir2")
            // random buck-out paths that might be being used (for installing .rpms)
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"))
            .inputs((Path::new("/tmp/rpmspec"), rpm_spec_file.path()))
            .inputs((Path::new("/__antlir2__/root"), self.layer.as_path()))
            .outputs((Path::new("/__antlir2__/out"), output_dir.path()))
            .tmpfs(Path::new("/tmp"))
            .tmpfs(Path::new("/dev"))
            .inputs(Path::new("/dev/null"));
        let isol_context = isol_context.build();

        run_cmd(
            unshare(isol_context.clone())?
                .command("/bin/rpmbuild")?
                .arg("-bb")
                .arg("--define")
                .arg("_rpmdir /__antlir2__/out")
                .arg("--define")
                .arg("_topdir /tmp/rpmbuild/top")
                .arg("--define")
                .arg("_tmppath /tmp/rpmbuild/tmp")
                .arg("/tmp/rpmspec")
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

        if let Some(privkey) = &self.sign_with_private_key {
            let rpm_path = outputs[0].path();
            let mut isol_context = IsolationContext::builder(&self.build_appliance);
            isol_context
                .ephemeral(false)
                .readonly()
                .hostname("antlir2")
                .working_directory(Path::new("/__antlir2__/working_directory"))
                .tmpfs(Path::new("/tmp"))
                .inputs(("/tmp/privkey", privkey.as_path()))
                .outputs(("/tmp/rpm", rpm_path.as_path()))
                .tmpfs(Path::new("/dev"))
                .inputs(Path::new("/dev/null"));
            let isol_context = isol_context.build();
            run_cmd(
                unshare(isol_context)?
                    .command("bash")?
                    .arg("-c")
                    .arg(format!(r#"
                set -ex

                export GNUPGHOME="/tmp/gpghome"
                mkdir "$GNUPGHOME"

                gpg --import /tmp/privkey
                keyid="$(gpg --show-keys --with-colons /tmp/privkey | awk -F':' '$1=="fpr"{{{{print $10}}}}' | head -1)"
                rpmsign --key-id "$keyid" {maybe_digest_algo} --addsign /tmp/rpm
                    "#,
                    maybe_digest_algo = self.sign_digest_algo.as_ref().map(|a| format!("--digest-algo {a}")).unwrap_or_default(),
                ))
                    .stdout(Stdio::piped()),
            )
            .context("failed to sign rpm")?;
        }

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
