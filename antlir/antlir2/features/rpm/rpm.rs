/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs::Permissions;
use std::io::Seek;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

use antlir2_compile::plan;
use antlir2_compile::plan::DnfTransaction;
use antlir2_compile::Arch;
use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::Requirement;
use antlir2_features::types::BuckOutSource;
use antlir2_isolate::unshare;
use antlir2_isolate::IsolationContext;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use buck_label::Label;
use serde::de::Error as _;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Deserializer;
use tempfile::NamedTempFile;
use tracing::trace;

pub type Feature = Rpm;

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
    Upgrade,
    Remove,
    RemoveIfExists,
    ModuleEnable,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Subject(String),
    #[serde(rename = "src")]
    Source(BuckOutSource),
    #[serde(rename = "subjects_src")]
    SubjectsSource(BuckOutSource),
}

/// Buck2's `record` will always include `null` values, but serde's native enum
/// deserialization will fail if there are multiple keys, even if the others are
/// null.
/// TODO(vmagro): make this general in the future (either codegen from `record`s
/// or as a proc-macro)
impl<'de> Deserialize<'de> for Source {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct SourceStruct {
            subject: Option<String>,
            src: Option<BuckOutSource>,
            subjects_src: Option<BuckOutSource>,
        }

        SourceStruct::deserialize(deserializer).and_then(|s| {
            match (s.subject, s.src, s.subjects_src) {
                (Some(subj), None, None) => Ok(Self::Subject(subj)),
                (None, Some(source), None) => Ok(Self::Source(source)),
                (None, None, Some(subjects_src)) => Ok(Self::SubjectsSource(subjects_src)),
                _ => Err(D::Error::custom(
                    "exactly one of {subject, src, subjects_src} must be set",
                )),
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct RpmItem {
    pub action: Action,
    pub rpm: Source,
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
pub struct Rpm {
    pub items: Vec<RpmItem>,
    #[serde(skip_deserializing)]
    pub internal_only_options: InternalOnlyOptions,
}

#[derive(
    Debug,
    Default,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
pub struct InternalOnlyOptions {
    #[serde(default)]
    pub ignore_scriptlet_errors: bool,
}

impl antlir2_depgraph_if::RequiresProvides for Rpm {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(Default::default())
    }
}

impl antlir2_compile::CompileFeature for Rpm {
    #[tracing::instrument(name = "rpms", skip(self, ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        run_dnf_driver(
            ctx,
            &self.items,
            DriverMode::Run,
            ctx.plan()
                .expect("rpms feature is always planned")
                .dnf_transaction()
                .cloned(),
            &self.internal_only_options,
        )
        .map(|_| ())
        .map_err(antlir2_compile::Error::from)
    }

    #[tracing::instrument(name = "rpms[plan]", skip(self, ctx), ret, err)]
    fn plan(&self, ctx: &CompilerContext) -> antlir2_compile::Result<Vec<plan::Item>> {
        let events = run_dnf_driver(
            ctx,
            &self.items,
            DriverMode::Resolve,
            None,
            &self.internal_only_options,
        )?;
        if events.len() != 1 {
            return Err(Error::msg("expected exactly one event in resolve-only mode").into());
        }
        match &events[0] {
            DriverEvent::TransactionResolved {
                install,
                remove,
                module_enable,
            } => Ok(vec![plan::Item::DnfTransaction(DnfTransaction {
                install: install
                    .iter()
                    .map(|ip| plan::InstallPackage {
                        nevra: ip.package.nevra(),
                        repo: ip.repo.clone(),
                        reason: ip.reason,
                    })
                    .collect(),
                remove: remove.iter().map(|p| p.nevra()).collect(),
                module_enable: module_enable.clone(),
            })]),
            _ => Err(Error::msg("resolve-only event should have been TransactionResolved").into()),
        }
    }
}

#[derive(Debug, Serialize)]
struct DriverSpec<'a> {
    repos: Option<&'a Path>,
    install_root: &'a Path,
    items: &'a [RpmItem],
    mode: DriverMode,
    arch: Arch,
    versionlock: &'a BTreeMap<String, String>,
    excluded_rpms: Option<&'a BTreeSet<String>>,
    resolved_transaction: Option<DnfTransaction>,
    ignore_scriptlet_errors: bool,
    layer_label: Label,
}

#[derive(Debug, Copy, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DriverMode {
    Resolve,
    Run,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionOperation {
    /// Package cleanup is being performed.
    Cleanup,
    /// Package is being installed as a downgrade
    Downgrade,
    /// Installed package is being downgraded
    Downgraded,
    /// Package is being installed
    Install,
    /// Package is obsoleting another package
    Obsolete,
    /// Installed package is being obsoleted
    Obsoleted,
    /// Package is installed as a reinstall
    Reinstall,
    /// Installed package is being reinstalled
    Reinstalled,
    /// Package is being removed
    Remove,
    /// Package is installed as an upgrade
    Upgrade,
    /// Installed package is being upgraded
    Upgraded,
    /// Package is being verified
    Verify,
    /// Package scriptlet is being performed
    Scriptlet,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
struct Package {
    name: String,
    epoch: i32,
    version: String,
    release: String,
    arch: String,
}

impl Package {
    fn nevra(&self) -> String {
        format!(
            "{}-{}:{}-{}.{}",
            self.name, self.epoch, self.version, self.release, self.arch
        )
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
struct InstallPackage {
    package: Package,
    repo: Option<String>,
    reason: plan::RpmReason,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[allow(dead_code)] // I want to log structured data
enum DriverEvent {
    TransactionResolved {
        install: BTreeSet<InstallPackage>,
        remove: BTreeSet<Package>,
        module_enable: BTreeSet<String>,
    },
    TxItem {
        package: Package,
        operation: TransactionOperation,
    },
    TxError(String),
    // TODO(T179574053) figure out some way to propagate this back for the user
    // to see...
    TxWarning(String),
    GpgError {
        package: Package,
        error: String,
    },
    ScriptletOutput(String),
    PackageNotFound(String),
    PackageNotInstalled(String),
}

fn run_dnf_driver(
    ctx: &CompilerContext,
    items: &[RpmItem],
    mode: DriverMode,
    resolved_transaction: Option<DnfTransaction>,
    internal_only_options: &InternalOnlyOptions,
) -> Result<Vec<DriverEvent>> {
    let items = items
        .iter()
        .cloned()
        .map(|item| match item.rpm {
            Source::SubjectsSource(subjects_src) => Ok(std::fs::read_to_string(&subjects_src)
                .with_context(|| format!("while reading {}", subjects_src.display()))?
                .lines()
                .map(|subject| RpmItem {
                    action: item.action,
                    rpm: Source::Subject(subject.to_owned()),
                    feature_label: item.feature_label.clone(),
                })
                .collect()),
            _ => Ok(vec![item]),
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let spec = DriverSpec {
        repos: Some(ctx.dnf().repos()),
        install_root: Path::new("/__antlir2__/root"),
        items: &items,
        mode,
        arch: ctx.target_arch(),
        versionlock: ctx.dnf().versionlock(),
        excluded_rpms: Some(ctx.dnf().excluded_rpms()),
        resolved_transaction,
        ignore_scriptlet_errors: internal_only_options.ignore_scriptlet_errors,
        layer_label: ctx.label().clone(),
    };

    // We can have per-OS rpm macros to change the database backend that must be
    // copied into the built image.
    let ba_macros = ctx.build_appliance().join("etc/rpm/macros.db");
    if ba_macros.exists() {
        let db_macro_path = ctx.dst_path("/etc/rpm/macros.db")?;
        // If the macros.db file already exists, just use it as-is. Most likely
        // it will have come from antlir2 in a parent_layer, but we also want to
        // allow images to override it if they want
        if !db_macro_path.exists() {
            std::fs::create_dir_all(db_macro_path.parent().expect("always has parent"))
                .with_context(|| format!("while creating dir for {}", db_macro_path.display()))?;
            std::fs::copy(ba_macros, &db_macro_path).with_context(|| {
                format!("while installing db macro {}", db_macro_path.display())
            })?;
        }
    }

    let opts = memfd::MemfdOptions::default().close_on_exec(false);
    let mfd = opts.create("input").context("while creating memfd")?;
    serde_json::to_writer(&mut mfd.as_file(), &spec)
        .context("while serializing dnf-driver input")?;
    mfd.as_file().rewind()?;

    let mut driver = NamedTempFile::new()?;
    driver
        .as_file()
        .set_permissions(Permissions::from_mode(0o555))?;
    driver.write_all(include_bytes!("./driver.py"))?;

    let isol = IsolationContext::builder(ctx.build_appliance())
        .ephemeral(false)
        .readonly()
        // random buck-out paths that might be being used (for installing .rpms)
        .inputs((
            PathBuf::from("/__antlir2__/working_directory"),
            std::env::current_dir()?,
        ))
        .working_directory(Path::new("/__antlir2__/working_directory"))
        .outputs((Path::new("/__antlir2__/root"), ctx.root()))
        .tmpfs(Path::new("/__antlir2__/dnf/cache"))
        .tmpfs(Path::new("/var/log"))
        .tmpfs(Path::new("/dev"))
        .tmpfs(Path::new("/tmp"))
        // TMPDIR might be set by buck2, be very explicit that it shouldn't be
        // inherited
        .setenv(("TMPDIR", "/tmp"))
        // even though the build appliance is mounted readonly, python is still
        // somehow writing .pyc cache files, just ban it
        .setenv(("PYTHONDONTWRITEBYTECODE", "1"))
        .inputs((Path::new("/tmp/dnf-driver"), driver.path()))
        .build();
    let isol = unshare(isol)?;

    let mut cmd = isol.command("/usr/libexec/platform-python")?;
    cmd.arg("/tmp/dnf-driver");
    trace!("dnf driver command: {cmd:#?}");

    let mut child = cmd
        .stdin(mfd.into_file())
        .stdout(Stdio::piped())
        .spawn()
        .inspect_err(|e| trace!("Spawning dnf-driver failed: {e}"))
        .context("while spawning dnf-driver")?;

    let deser = Deserializer::from_reader(child.stdout.take().expect("this is a pipe"));
    let mut events = Vec::new();
    for event in deser.into_iter::<DriverEvent>() {
        let event = event.context("while deserializing event from dnf-driver")?;
        trace!("dnf-driver: {event:?}");
        events.push(event);
    }
    let result = child.wait().context("while waiting for dnf-driver")?;

    if !result.success() {
        Err(Error::msg("dnf-driver failed"))
    } else {
        // make sure there weren't any error events, if there was -> fail
        let errors: Vec<_> = events
            .iter()
            .filter_map(|ev| match ev {
                DriverEvent::TxError(error) => Some(Cow::Borrowed(error.as_str())),
                DriverEvent::PackageNotFound(package) => {
                    Some(Cow::Owned(format!("No such package found '{package}'")))
                }
                DriverEvent::PackageNotInstalled(package) => Some(Cow::Owned(format!(
                    "Package to be removed '{package}' was not installed"
                ))),
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
