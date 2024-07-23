/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs::Permissions;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Seek;
use std::io::Write;
use std::ops::Deref;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

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
use json_arg::JsonFile;
use serde::de::Error as _;
use serde::ser::SerializeStruct;
use serde::ser::Serializer;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Deserializer;
use tempfile::NamedTempFile;
use tempfile::TempDir;
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
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    #[serde(rename = "tx_file")]
    tx: JsonFile<ResolvedTransaction>,
    build_appliance: PathBuf,
    repos: PathBuf,
    versionlock: Option<JsonFile<HashMap<String, String>>>,
    versionlock_extend: HashMap<String, String>,
    excluded_rpms: BTreeSet<String>,
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
    #[tracing::instrument(name = "rpms", skip(self, ctx), ret, err(Debug))]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let plan: Plan = ctx
            .plan("rpm")
            .context("rpm feature was not planned")?
            .context("while loading rpm plan")?;
        run_dnf_driver(
            DriverContext::Compile {
                ctx,
                build_appliance: plan.build_appliance,
                repos: plan.repos,
                versionlock: plan
                    .versionlock
                    .map(JsonFile::into_inner)
                    .unwrap_or_default()
                    .into_iter()
                    .chain(plan.versionlock_extend.into_iter())
                    .collect(),
                excluded_rpms: plan.excluded_rpms,
            },
            &self.items,
            DriverMode::Run,
            Some(plan.tx.into_inner()),
            &self.internal_only_options,
        )
        .map(|_| ())
        .map_err(antlir2_compile::Error::from)
    }
}

impl Rpm {
    #[tracing::instrument(skip_all)]
    pub fn plan(&self, ctx: DriverContext) -> anyhow::Result<ResolvedTransaction, Error> {
        let mut events = run_dnf_driver(
            ctx,
            #[allow(unreachable_code)]
            &self.items,
            DriverMode::Resolve,
            None,
            &Default::default(),
        )?;
        if events.len() != 1 {
            return Err(Error::msg(
                "expected exactly one event in resolve-only mode",
            ));
        }
        if let DriverEvent::TransactionResolved {
            install,
            remove,
            module_enable,
        } = events.remove(0)
        {
            Ok(ResolvedTransaction {
                install,
                remove: remove.iter().map(Package::nevra).collect(),
                module_enable,
            })
        } else {
            Err(Error::msg(
                "resolve-only event should have been TransactionResolved",
            ))
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
    excluded_rpms: &'a BTreeSet<String>,
    resolved_transaction: Option<ResolvedTransaction>,
    ignore_scriptlet_errors: bool,
    layer_label: Label,
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
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

#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Clean,
    Dependency,
    Group,
    Unknown,
    User,
    WeakDependency,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
struct InstallPackage {
    package: Package,
    repo: Option<String>,
    reason: Reason,
}

impl Serialize for InstallPackage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("InstallPackage", 4)?;
        state.serialize_field("nevra", &self.package.nevra())?;
        state.serialize_field("package", &self.package)?;
        state.serialize_field("repo", &self.repo)?;
        state.serialize_field("reason", &self.reason)?;
        state.end()
    }
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
    GpgWarning {
        package: Package,
        error: String,
    },
    ScriptletOutput(String),
    PackageNotFound(String),
    PackageNotInstalled(String),
}

pub enum DriverContext<'a> {
    Compile {
        ctx: &'a CompilerContext,
        build_appliance: PathBuf,
        repos: PathBuf,
        versionlock: BTreeMap<String, String>,
        excluded_rpms: BTreeSet<String>,
    },
    Plan {
        label: Label,
        root: Option<PathBuf>,
        build_appliance: PathBuf,
        repos: PathBuf,
        target_arch: Arch,
        versionlock: BTreeMap<String, String>,
        excluded_rpms: BTreeSet<String>,
    },
}

impl DriverContext<'_> {
    pub fn plan(
        label: Label,
        root: Option<PathBuf>,
        build_appliance: PathBuf,
        repos: PathBuf,
        target_arch: Arch,
        versionlock: BTreeMap<String, String>,
        excluded_rpms: BTreeSet<String>,
    ) -> Self {
        Self::Plan {
            label,
            root,
            build_appliance,
            repos,
            target_arch,
            versionlock,
            excluded_rpms,
        }
    }

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

    fn repos(&self) -> &Path {
        match self {
            Self::Plan { repos, .. } => repos,
            Self::Compile { repos, .. } => repos,
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
            Self::Plan { root, .. } => root.as_deref(),
            Self::Compile { ctx, .. } => Some(ctx.root_path()),
        }
    }

    fn versionlock(&self) -> &BTreeMap<String, String> {
        match self {
            Self::Plan { versionlock, .. } => versionlock,
            Self::Compile { versionlock, .. } => versionlock,
        }
    }

    fn excluded_rpms(&self) -> &BTreeSet<String> {
        match self {
            Self::Plan { excluded_rpms, .. } => excluded_rpms,
            Self::Compile { excluded_rpms, .. } => excluded_rpms,
        }
    }

    fn is_planning(&self) -> bool {
        match self {
            Self::Plan { .. } => true,
            _ => false,
        }
    }
}

enum Root {
    Empty(TempDir),
    Root(PathBuf),
}

impl Deref for Root {
    type Target = Path;

    fn deref(&self) -> &Path {
        match self {
            Self::Empty(tmp) => tmp.path(),
            Self::Root(root) => root,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResolvedTransaction {
    install: BTreeSet<InstallPackage>,
    remove: BTreeSet<String>,
    module_enable: BTreeSet<String>,
}

fn run_dnf_driver(
    ctx: DriverContext,
    items: &[RpmItem],
    mode: DriverMode,
    resolved_transaction: Option<ResolvedTransaction>,
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
        repos: Some(ctx.repos()),
        install_root: Path::new("/__antlir2__/root"),
        items: &items,
        mode,
        arch: ctx.target_arch(),
        versionlock: ctx.versionlock(),
        excluded_rpms: ctx.excluded_rpms(),
        resolved_transaction,
        ignore_scriptlet_errors: internal_only_options.ignore_scriptlet_errors,
        layer_label: ctx.label().clone(),
    };

    let root = match ctx.root_path() {
        Some(r) => Root::Root(r.to_owned()),
        None => Root::Empty(TempDir::new().context("while creating empty root dir")?),
    };

    // Don't mess with db macros while planning a transaction, we should instead
    // only use what is already there (plus, during planning the installroot is
    // readonly and we can't actually create this)
    if !ctx.is_planning() {
        // We can have per-OS rpm macros to change the database backend that must be
        // copied into the built image.
        let ba_macros = ctx.build_appliance().join("etc/rpm/macros.db");
        if ba_macros.exists() {
            let db_macro_path = root.join("etc/rpm/macros.db");
            // If the macros.db file already exists, just use it as-is. Most likely
            // it will have come from antlir2 in a parent_layer, but we also want to
            // allow images to override it if they want
            if !db_macro_path.exists() {
                std::fs::create_dir_all(db_macro_path.parent().expect("always has parent"))
                    .with_context(|| {
                        format!("while creating dir for {}", db_macro_path.display())
                    })?;
                std::fs::copy(ba_macros, &db_macro_path).with_context(|| {
                    format!("while installing db macro {}", db_macro_path.display())
                })?;
            }
        }
    }

    let opts = memfd::MemfdOptions::default().close_on_exec(false);
    let mfd = opts.create("input").context("while creating memfd")?;
    serde_json::to_writer(BufWriter::new(mfd.as_file()), &spec)
        .context("while serializing dnf-driver input")?;
    mfd.as_file().rewind()?;

    let mut driver = NamedTempFile::new()?;
    driver
        .as_file()
        .set_permissions(Permissions::from_mode(0o555))?;
    driver.write_all(include_bytes!("./driver.py"))?;

    let mut isol = IsolationContext::builder(ctx.build_appliance());
    isol.ephemeral(false)
        .readonly()
        // random buck-out paths that might be being used (for installing .rpms)
        .inputs((
            PathBuf::from("/__antlir2__/working_directory"),
            std::env::current_dir()?,
        ))
        .working_directory(Path::new("/__antlir2__/working_directory"))
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
    if ctx.is_planning() {
        isol.inputs((Path::new("/__antlir2__/root"), root.deref()))
            .tmpfs_overlay(Path::new("/__antlir2__/root"));
    } else {
        isol.outputs((Path::new("/__antlir2__/root"), root.deref()));
    }

    let isol = unshare(isol.build())?;

    let mut cmd = isol.command("/usr/libexec/platform-python")?;
    cmd.arg("/tmp/dnf-driver");
    trace!("dnf driver command: {cmd:#?}");

    let mut child = cmd
        .stdin(mfd.into_file())
        .stdout(Stdio::piped())
        .spawn()
        .inspect_err(|e| trace!("Spawning dnf-driver failed: {e}"))
        .context("while spawning dnf-driver")?;

    let deser =
        Deserializer::from_reader(BufReader::new(child.stdout.take().expect("this is a pipe")));
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
