/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Seek;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

use antlir2_compile::Arch;
use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::item::Item;
// Mandatory dep from feature_impl(); suppress unused-dep check.
use antlir2_features as _;
use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use buck_label::Label;
use json_arg::JsonFile;
use serde::Deserialize;
use serde::Serialize;
use serde::de::Error as _;
use serde_json::Deserializer;
use tracing::trace;

pub type Feature = Apt;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Install,
    Remove,
    RemoveIfExists,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AptSource {
    Subject(String),
}

/// Buck2's `record` will always include `null` values, but serde's native enum
/// deserialization will fail if there are multiple keys, even if the others are
/// null.
impl<'de> Deserialize<'de> for AptSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct SourceStruct {
            subject: Option<String>,
        }

        SourceStruct::deserialize(deserializer).and_then(|s| match s.subject {
            Some(subj) => Ok(Self::Subject(subj)),
            None => Err(D::Error::custom("subject must be set")),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct AptItem {
    pub action: Action,
    pub apt: AptSource,
    pub feature_label: Label,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize,
    Default
)]
#[serde(deny_unknown_fields)]
pub struct Apt {
    pub items: Vec<AptItem>,
    pub driver_cmd: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    #[serde(rename = "tx_file")]
    tx: JsonFile<ResolvedTransaction>,
    build_appliance: PathBuf,
    archive_dir: PathBuf,
    debs_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub filename: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct InstallPackage {
    pub package: Package,
    pub archive: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResolvedTransaction {
    pub install: BTreeSet<InstallPackage>,
    pub remove: BTreeSet<RemovePackage>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct RemovePackage {
    pub name: String,
    pub version: String,
    pub arch: String,
}

impl antlir2_depgraph_if::RequiresProvides for Apt {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(Default::default())
    }
}

impl antlir2_compile::CompileFeature for Apt {
    #[tracing::instrument(name = "apt", skip(self, ctx), ret, err(Debug))]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let plan: Plan = ctx
            .plan("apt")
            .context("apt feature was not planned")?
            .context("while loading apt plan")?;
        run_apt_driver(
            DriverContext::Compile {
                ctx,
                build_appliance: plan.build_appliance,
                archive_dir: plan.archive_dir,
                debs_dir: plan.debs_dir,
            },
            &self.driver_cmd,
            &self.items,
            DriverMode::Run,
            Some(plan.tx.into_inner()),
        )
        .map(|_| ())
        .map_err(antlir2_compile::Error::from)
    }
}

impl Apt {
    #[tracing::instrument(skip_all)]
    pub fn plan(
        &self,
        ctx: DriverContext,
    ) -> anyhow::Result<(ResolvedTransaction, Option<String>)> {
        let mut events = run_apt_driver(
            ctx,
            &self.driver_cmd,
            &self.items,
            DriverMode::Resolve,
            None,
        )?;
        if events.len() != 1 {
            return Err(Error::msg(
                "expected exactly one event in resolve-only mode",
            ));
        }
        if let DriverEvent::TransactionResolved {
            install,
            remove,
            dpkg_status,
        } = events.remove(0)
        {
            Ok((ResolvedTransaction { install, remove }, dpkg_status))
        } else {
            Err(Error::msg(
                "resolve-only event should have been TransactionResolved",
            ))
        }
    }
}

#[derive(Debug, Serialize)]
struct DriverSpec<'a> {
    archive_dir: &'a Path,
    install_root: &'a Path,
    items: &'a [AptItem],
    mode: DriverMode,
    arch: &'a str,
    resolved_transaction: Option<ResolvedTransaction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dpkg_status: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    debs_dir: Option<&'a Path>,
}

#[derive(Debug, Copy, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriverMode {
    Resolve,
    Run,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionOperation {
    Install,
    Remove,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[allow(dead_code)]
enum DriverEvent {
    TransactionResolved {
        install: BTreeSet<InstallPackage>,
        remove: BTreeSet<RemovePackage>,
        #[serde(default)]
        dpkg_status: Option<String>,
    },
    TxItem {
        package: serde_json::Value,
        operation: TransactionOperation,
    },
    TxError(String),
    TxWarning(String),
    ScriptletOutput(String),
    PackageNotFound(String),
    PackageNotInstalled(String),
}

pub enum DriverContext<'a> {
    Compile {
        ctx: &'a CompilerContext,
        build_appliance: PathBuf,
        archive_dir: PathBuf,
        debs_dir: PathBuf,
    },
    Plan {
        label: Label,
        build_appliance: PathBuf,
        archive_dir: PathBuf,
        target_arch: Arch,
        dpkg_status: Option<String>,
    },
}

impl DriverContext<'_> {
    pub fn plan(
        label: Label,
        build_appliance: PathBuf,
        archive_dir: PathBuf,
        target_arch: Arch,
        dpkg_status: Option<String>,
    ) -> Self {
        Self::Plan {
            label,
            build_appliance,
            archive_dir,
            target_arch,
            dpkg_status,
        }
    }

    #[allow(dead_code)]
    fn label(&self) -> &Label {
        match self {
            Self::Plan { label, .. } => label,
            Self::Compile { ctx, .. } => ctx.label(),
        }
    }

    fn build_appliance(&self) -> &Path {
        match self {
            Self::Plan {
                build_appliance, ..
            } => build_appliance,
            Self::Compile {
                build_appliance, ..
            } => build_appliance,
        }
    }

    fn archive_dir(&self) -> &Path {
        match self {
            Self::Plan { archive_dir, .. } => archive_dir,
            Self::Compile { archive_dir, .. } => archive_dir,
        }
    }

    fn target_arch(&self) -> Arch {
        match self {
            Self::Plan { target_arch, .. } => *target_arch,
            Self::Compile { ctx, .. } => ctx.target_arch(),
        }
    }

    fn root_path(&self) -> Option<&Path> {
        match self {
            Self::Plan { .. } => None,
            Self::Compile { ctx, .. } => Some(ctx.root_path()),
        }
    }

    fn is_planning(&self) -> bool {
        matches!(self, Self::Plan { .. })
    }

    fn dpkg_status(&self) -> Option<&str> {
        match self {
            Self::Plan { dpkg_status, .. } => dpkg_status.as_deref(),
            Self::Compile { .. } => None,
        }
    }

    fn debs_dir(&self) -> Option<&Path> {
        match self {
            Self::Compile { debs_dir, .. } => Some(debs_dir),
            Self::Plan { .. } => None,
        }
    }
}

fn arch_to_deb_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::X86_64 => "amd64",
        Arch::Aarch64 => "arm64",
    }
}

fn run_apt_driver(
    ctx: DriverContext,
    driver: &[String],
    items: &[AptItem],
    mode: DriverMode,
    resolved_transaction: Option<ResolvedTransaction>,
) -> Result<Vec<DriverEvent>> {
    // The archive dir is a symlinked_dir with relative symlinks that resolve
    // within the working directory tree. Mount the working directory and
    // reference the archive through it so symlinks resolve correctly.
    let cwd = std::env::current_dir()?;
    let archive_in_container = Path::new("/__antlir2__/working_directory").join(ctx.archive_dir());
    let debs_dir_in_container = ctx
        .debs_dir()
        .map(|d| Path::new("/__antlir2__/working_directory").join(d));

    let spec = DriverSpec {
        archive_dir: &archive_in_container,
        install_root: Path::new("/__antlir2__/root"),
        items,
        mode,
        arch: arch_to_deb_arch(ctx.target_arch()),
        resolved_transaction,
        dpkg_status: ctx.dpkg_status(),
        debs_dir: debs_dir_in_container.as_deref(),
    };

    let opts = memfd::MemfdOptions::default().close_on_exec(false);
    let mfd = opts.create("input").context("while creating memfd")?;
    serde_json::to_writer(BufWriter::new(mfd.as_file()), &spec)
        .context("while serializing apt-driver input")?;
    mfd.as_file().rewind()?;

    let mut isol = IsolationContext::builder(ctx.build_appliance());
    isol.ephemeral(false)
        .readonly()
        .inputs((PathBuf::from("/__antlir2__/working_directory"), cwd))
        .working_directory(Path::new("/__antlir2__/working_directory"))
        .tmpfs(Path::new("/var/log"))
        .tmpfs(Path::new("/dev"))
        .tmpfs(Path::new("/tmp"))
        .setenv(("TMPDIR", "/tmp"))
        .setenv(("PYTHONDONTWRITEBYTECODE", "1"));
    if ctx.is_planning() {
        isol.tmpfs(Path::new("/__antlir2__/root"));
    } else {
        let root = ctx.root_path().expect("compile context must have a root");
        isol.outputs((Path::new("/__antlir2__/root"), root));
    }

    let isol = unshare(isol.build())?;

    let mut driver_cmd = driver.iter();

    let mut cmd = isol.command(driver_cmd.next().context("driver_cmd is empty")?)?;
    cmd.args(driver_cmd);

    let mut child = cmd
        .stdin(mfd.into_file())
        .stdout(Stdio::piped())
        .spawn()
        .inspect_err(|e| trace!("Spawning apt-driver failed: {e}"))
        .context("while spawning apt-driver")?;

    let deser =
        Deserializer::from_reader(BufReader::new(child.stdout.take().expect("this is a pipe")));
    let mut events = Vec::new();
    for event in deser.into_iter::<DriverEvent>() {
        let event = event.context("while deserializing event from apt-driver")?;
        trace!("apt-driver: {event:?}");
        events.push(event);
    }
    let result = child.wait().context("while waiting for apt-driver")?;

    if !result.success() {
        Err(Error::msg("apt-driver failed"))
    } else {
        let errors: Vec<_> = events
            .iter()
            .filter_map(|ev| match ev {
                DriverEvent::TxError(error) => Some(error.clone()),
                DriverEvent::PackageNotFound(package) => {
                    Some(format!("No such package found '{package}'"))
                }
                DriverEvent::PackageNotInstalled(package) => Some(format!(
                    "Package to be removed '{package}' was not installed"
                )),
                _ => None,
            })
            .collect();
        if !errors.is_empty() {
            return Err(anyhow::anyhow!(
                "there were one or more transaction errors: {errors:?}"
            ));
        }
        Ok(events)
    }
}
