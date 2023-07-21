/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use antlir2_compile::plan;
use antlir2_compile::plan::DnfTransaction;
use antlir2_compile::Arch;
use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_features::types::BuckOutSource;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use buck_label::Label;
use serde::de::Error as _;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Deserializer;
use tracing::trace;

pub type Feature = Rpm<'static>;

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
    RemoveIfExists,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case", bound(deserialize = "'de: 'a"))]
pub enum Source<'a> {
    Subject(Cow<'a, str>),
    #[serde(rename = "src")]
    Source(BuckOutSource<'a>),
    #[serde(rename = "subjects_src")]
    SubjectsSource(BuckOutSource<'a>),
}

/// Buck2's `record` will always include `null` values, but serde's native enum
/// deserialization will fail if there are multiple keys, even if the others are
/// null.
/// TODO(vmagro): make this general in the future (either codegen from `record`s
/// or as a proc-macro)
impl<'a, 'de: 'a> Deserialize<'de> for Source<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "'de: 'a"))]
        struct SourceStruct<'a> {
            subject: Option<Cow<'a, str>>,
            src: Option<BuckOutSource<'a>>,
            subjects_src: Option<BuckOutSource<'a>>,
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
#[serde(bound(deserialize = "'de: 'a"))]
pub struct RpmItem<'a> {
    pub action: Action,
    pub rpm: Source<'a>,
    pub feature_label: Label<'a>,
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
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Rpm<'a> {
    pub items: Vec<RpmItem<'a>>,
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
    pub ignore_postin_script_error: bool,
}

impl<'f> antlir2_feature_impl::Feature<'f> for Rpm<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement<'f>>> {
        Ok(Default::default())
    }

    #[tracing::instrument(name = "rpms", skip(self, ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
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
    }

    #[tracing::instrument(name = "rpms[plan]", skip(self, ctx), ret, err)]
    fn plan(&self, ctx: &CompilerContext) -> Result<Vec<plan::Item>> {
        let events = run_dnf_driver(
            ctx,
            &self.items,
            DriverMode::Resolve,
            None,
            &self.internal_only_options,
        )?;
        if events.len() != 1 {
            return Err(Error::msg(
                "expected exactly one event in resolve-only mode",
            ));
        }
        match &events[0] {
            DriverEvent::TransactionResolved { install, remove } => {
                Ok(vec![plan::Item::DnfTransaction(DnfTransaction {
                    install: install
                        .iter()
                        .map(|ip| plan::InstallPackage {
                            nevra: ip.package.nevra(),
                            repo: ip.repo.clone(),
                            reason: ip.reason,
                        })
                        .collect(),
                    remove: remove.iter().map(|p| p.nevra()).collect(),
                })])
            }
            _ => Err(Error::msg(
                "resolve-only event should have been TransactionResolved",
            )),
        }
    }
}

#[derive(Debug, Serialize)]
struct DriverSpec<'a> {
    repos: Option<&'a Path>,
    install_root: &'a Path,
    items: &'a [RpmItem<'a>],
    mode: DriverMode,
    arch: Arch,
    versionlock: Option<&'a BTreeMap<String, String>>,
    excluded_rpms: Option<&'a BTreeSet<String>>,
    resolved_transaction: Option<DnfTransaction>,
    ignore_postin_script_error: bool,
    layer_label: Label<'a>,
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
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // I want to log structured data
enum DriverEvent {
    TransactionResolved {
        install: BTreeSet<InstallPackage>,
        remove: BTreeSet<Package>,
    },
    TxItem {
        package: Package,
        operation: TransactionOperation,
    },
    TxError(String),
    TxWarning(String),
    GpgError {
        package: Package,
        error: String,
    },
    ScriptletOutput(String),
}

fn run_dnf_driver(
    ctx: &CompilerContext,
    items: &[RpmItem<'_>],
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
                    rpm: Source::Subject(subject.to_owned().into()),
                    feature_label: item.feature_label.clone(),
                })
                .collect()),
            _ => Ok(vec![item]),
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let input = serde_json::to_string(&DriverSpec {
        repos: Some(ctx.dnf().repos()),
        install_root: ctx.root(),
        items: &items,
        mode,
        arch: ctx.target_arch(),
        versionlock: ctx.dnf().versionlock(),
        excluded_rpms: Some(ctx.dnf().excluded_rpms()),
        resolved_transaction,
        ignore_postin_script_error: internal_only_options.ignore_postin_script_error,
        layer_label: ctx.label().clone(),
    })
    .context("while serializing dnf-driver input")?;

    let mut child = Command::new("/__antlir2__/dnf/driver")
        .arg(&input)
        .stdout(Stdio::piped())
        .spawn()
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
                DriverEvent::TxError(error) => Some(error.as_str()),
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
