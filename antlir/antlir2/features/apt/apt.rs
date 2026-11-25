/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fs::Permissions;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::ErrorKind;
use std::io::Seek;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use antlir2_compile::Arch;
use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::item::Item;
use antlir2_features::types::BuckOutSource;
use antlir2_isolate::InvocationType;
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
use tracing::debug;
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
    /// A file of newline-separated sonames. Debian publishes no soname ->
    /// package mapping, so unlike an rpm capability these cannot be handed
    /// straight to the package manager; the resolver looks each one up.
    SonamesSource(BuckOutSource),
    /// A single soname, expanded from `SonamesSource` before the driver runs.
    Soname(String),
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
            sonames_src: Option<BuckOutSource>,
        }

        SourceStruct::deserialize(deserializer).and_then(|s| match (s.subject, s.sonames_src) {
            (Some(subj), None) => Ok(Self::Subject(subj)),
            (None, Some(src)) => Ok(Self::SonamesSource(src)),
            _ => Err(D::Error::custom(
                "exactly one of {subject, sonames_src} must be set",
            )),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct AptItem {
    pub action: Action,
    pub apt: AptSource,
    pub feature_label: Label,
    pub install_recommends: bool,
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
    /// SOURCE_DATE_EPOCH for maintainer scripts, as `apt-get` passes its own
    /// environment on to them
    #[serde(default)]
    pub source_date_epoch: Option<u64>,
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
    #[serde(default)]
    pub sha512: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct InstallPackage {
    pub package: Package,
    pub archive: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ResolvedTransaction {
    pub install: Vec<InstallPackage>,
    pub remove: Vec<RemovePackage>,
    /// The order apt has dpkg unpack the packages to install in
    #[serde(default)]
    pub unpack: Vec<String>,
    /// ... and configure as it unpacks, ahead of `dpkg --configure --pending`
    #[serde(default)]
    pub configure: Vec<String>,
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
        self.run_transaction(ctx, plan)
            .map_err(antlir2_compile::Error::from)
    }
}

const DPKG_INFO_DIR: &str = "/var/lib/dpkg/info";

const DPKG_TMP_CI_DIR: &str = "/var/lib/dpkg/tmp.ci";

/// Where the driver extracts the control files of the packages to install
const CONTROL_DIR: &str = "/var/lib/dpkg/antlir2-control";

impl Apt {
    /// Apply the transaction the way apt has dpkg do it: remove packages,
    /// running prerm and postrm around the removal of their files, unpack
    /// the new ones (`dpkg --unpack`), each right after its preinst, then
    /// configure them (`dpkg --configure --pending`).
    /// A new root has no /bin/sh for preinst until the essential packages
    /// are unpacked, so, like debootstrap, every package is extracted into
    /// it first. At the end, dpkg processes the triggers the transaction
    /// activated, as at the end of every dpkg run apt makes.
    fn run_transaction(&self, ctx: &CompilerContext, plan: Plan) -> Result<()> {
        self.apply_transaction(ctx, plan)?;
        run_pending_triggers(ctx, self.source_date_epoch)
    }

    fn apply_transaction(&self, ctx: &CompilerContext, plan: Plan) -> Result<()> {
        let tx = plan.tx.into_inner();
        let run_driver = |tx: ResolvedTransaction, stage: Option<DriverStage>| {
            run_apt_driver(
                DriverContext::Compile {
                    ctx,
                    build_appliance: plan.build_appliance.clone(),
                    archive_dir: plan.archive_dir.clone(),
                    debs_dir: plan.debs_dir.clone(),
                },
                &self.driver_cmd,
                &self.items,
                DriverMode::Run,
                Some(tx),
                stage,
            )
        };
        let script = |name: &str, arch: &str, script: &str, args: &[&str], dir: &str| {
            run_maintainer_script(
                ctx,
                MaintainerScript {
                    dir,
                    package: name,
                    arch,
                    script,
                    args,
                    source_date_epoch: self.source_date_epoch,
                },
            )
        };

        if !tx.remove.is_empty() {
            // dpkg removes dependents before the packages they depend on
            let remove: Vec<RemovePackage> = sort_by_dependencies(
                ctx,
                tx.remove.iter().map(|pkg| pkg.name.clone()).collect(),
                true,
            )?
            .into_iter()
            .map(|name| {
                tx.remove
                    .iter()
                    .find(|pkg| pkg.name == name)
                    .expect("sorted name came from tx.remove")
                    .clone()
            })
            .collect();
            for pkg in &remove {
                script(&pkg.name, &pkg.arch, "prerm", &["remove"], DPKG_INFO_DIR)?;
            }
            // The driver deletes all of a package's info files, but postrm
            // runs after the files are gone
            let mut postrms = Vec::new();
            for pkg in &remove {
                let mut path =
                    ctx.dst_path(format!("{DPKG_INFO_DIR}/{}:{}.postrm", pkg.name, pkg.arch))?;
                if !path.exists() {
                    path = ctx.dst_path(format!("{DPKG_INFO_DIR}/{}.postrm", pkg.name))?;
                }
                match std::fs::read(&path) {
                    Ok(contents) => postrms.push((pkg, path, contents)),
                    Err(e) if e.kind() == ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(e).with_context(|| format!("while reading {}", path.display()));
                    }
                }
            }
            run_driver(
                ResolvedTransaction {
                    remove: tx.remove.clone(),
                    ..Default::default()
                },
                None,
            )?;
            for (pkg, path, contents) in postrms {
                std::fs::write(&path, contents)
                    .with_context(|| format!("while restoring {}", path.display()))?;
                std::fs::set_permissions(&path, Permissions::from_mode(0o755))?;
                let res = script(&pkg.name, &pkg.arch, "postrm", &["remove"], DPKG_INFO_DIR);
                std::fs::remove_file(&path)
                    .with_context(|| format!("while removing {}", path.display()))?;
                res?;
            }
        }

        if tx.install.is_empty() {
            return Ok(());
        }
        let previous_versions = installed_versions(ctx)?;
        let install_tx = |packages: Vec<InstallPackage>| ResolvedTransaction {
            install: packages,
            ..Default::default()
        };
        let find = |name: &str| {
            tx.install
                .iter()
                .find(|p| p.package.name == name)
                .expect("sorted name came from tx.install")
                .clone()
        };
        let names: Vec<String> = tx.install.iter().map(|p| p.package.name.clone()).collect();

        run_driver(install_tx(tx.install.clone()), Some(DriverStage::Control))?;
        // A new root is installed as debootstrap and mkosi install it: the
        // files of the Essential packages and what they depend on are
        // extracted, those packages are unpacked and configured, and only
        // then are the rest unpacked and configured
        let phases = if previous_versions.is_empty() {
            let essential = control_relations(ctx, &names)?.essential_closure(&names);
            let extract = tx
                .install
                .iter()
                .filter(|p| essential.contains(&p.package.name))
                .cloned()
                .collect();
            run_driver(install_tx(extract), Some(DriverStage::Extract))?;
            let (first, rest): (Vec<String>, Vec<String>) =
                names.iter().cloned().partition(|n| essential.contains(n));
            vec![first, rest]
        } else {
            vec![names.clone()]
        };

        // dpkg unpacks in apt's order, and runs a package's preinst right
        // before it unpacks it
        let unpack = |names: &[String]| -> Result<()> {
            let mut batch = Vec::new();
            for name in apt_order(names, &tx.unpack) {
                let package = find(&name).package;
                let control = ctx.dst_path(format!("{CONTROL_DIR}/{name}"))?;
                if control.join("preinst").exists() {
                    if !batch.is_empty() {
                        run_driver(install_tx(std::mem::take(&mut batch)), None)?;
                    }
                    let args = match previous_versions.get(&package.name) {
                        Some(old) => vec!["upgrade", old.as_str()],
                        // dpkg passes the old version as $2, empty when there
                        // is none; `set -u` scripts (e.g. libselinux1) rely on
                        // it being set
                        None => vec!["install", ""],
                    };
                    // dpkg runs it from tmp.ci, which holds the package's control
                    // files (debconf reads the package name from tmp.ci/control)
                    let tmp_ci = ctx.dst_path(DPKG_TMP_CI_DIR)?;
                    std::fs::rename(&control, &tmp_ci)?;
                    let res = script(
                        &package.name,
                        &package.arch,
                        "preinst",
                        &args,
                        DPKG_TMP_CI_DIR,
                    );
                    std::fs::rename(&tmp_ci, &control)?;
                    res?;
                }
                batch.push(find(&name));
            }
            if !batch.is_empty() {
                run_driver(install_tx(batch), None)?;
            }
            Ok(())
        };

        // apt has dpkg configure what later packages pre-depend on as it
        // unpacks, and the rest with `dpkg --configure --pending`
        let configure = |names: &[String]| -> Result<()> {
            let relations = status_relations(ctx)?;
            let (explicit, pending): (Vec<String>, Vec<String>) = apt_order(names, &tx.configure)
                .into_iter()
                .partition(|n| tx.configure.iter().any(|c| bare_name(c) == bare_name(n)));
            for name in explicit
                .into_iter()
                .chain(dpkg_configure_order(&pending, &relations))
            {
                let package = find(&name).package;
                let args = match previous_versions.get(&package.name) {
                    Some(old) => vec!["configure", old.as_str()],
                    None => vec!["configure", ""],
                };
                script(
                    &package.name,
                    &package.arch,
                    "postinst",
                    &args,
                    DPKG_INFO_DIR,
                )?;
            }
            Ok(())
        };

        for phase in phases {
            unpack(&phase)?;
            configure(&phase)?;
        }
        Ok(())
    }
}

/// Versions of the packages currently recorded in the image's dpkg status db
fn installed_versions(ctx: &CompilerContext) -> Result<HashMap<String, String>> {
    let path = ctx.dst_path("/var/lib/dpkg/status")?;
    let status = match std::fs::read_to_string(&path) {
        Ok(status) => status,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e).with_context(|| format!("while reading {}", path.display())),
    };
    Ok(status
        .split("\n\n")
        .filter_map(|stanza| {
            let field = |name: &str| {
                stanza
                    .lines()
                    .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
                    .map(str::trim)
            };
            Some((field("Package")?.to_owned(), field("Version")?.to_owned()))
        })
        .collect())
}

struct MaintainerScript<'a> {
    /// where the script is: the info dir, or for preinst the new package's
    /// control files
    dir: &'a str,
    package: &'a str,
    arch: &'a str,
    script: &'a str,
    args: &'a [&'a str],
    source_date_epoch: Option<u64>,
}

/// Run a maintainer script (if the package has one) with the image as the
/// root filesystem
fn run_maintainer_script(ctx: &CompilerContext, s: MaintainerScript) -> Result<()> {
    let MaintainerScript {
        dir,
        package,
        arch,
        script,
        args,
        source_date_epoch,
    } = s;
    let path = if dir == DPKG_INFO_DIR {
        // a Multi-Arch: same package's info files are named with its arch
        let qualified = format!("{dir}/{package}:{arch}.{script}");
        if ctx.dst_path(&qualified)?.exists() {
            qualified
        } else {
            format!("{dir}/{package}.{script}")
        }
    } else {
        format!("{dir}/{script}")
    };
    if !ctx.dst_path(&path)?.exists() {
        return Ok(());
    }
    let mut cmd = image_command(
        ctx,
        &path,
        source_date_epoch,
        &[
            ("DPKG_MAINTSCRIPT_PACKAGE", package),
            ("DPKG_MAINTSCRIPT_NAME", script),
            ("DPKG_MAINTSCRIPT_ARCH", arch),
        ],
    )?;
    cmd.args(args);
    trace!("running maintainer script: {cmd:?}");
    run_image_command(cmd, &format!("{package} {script} {args:?}"))
}

/// Have dpkg process the triggers that are pending, as it does at the end of
/// every run apt makes: this also sets up dpkg's triggers database, without
/// which `dpkg-trigger` in later maintainer scripts records nothing. apt
/// holds dpkg's frontend lock while it runs dpkg, so dpkg takes none.
fn run_pending_triggers(ctx: &CompilerContext, source_date_epoch: Option<u64>) -> Result<()> {
    if !ctx.dst_path("/usr/bin/dpkg")?.exists() {
        return Ok(());
    }
    let mut cmd = image_command(
        ctx,
        "/usr/bin/dpkg",
        source_date_epoch,
        &[("DPKG_FRONTEND_LOCKED", "1")],
    )?;
    cmd.args(["--triggers-only", "--pending"]);
    trace!("processing pending triggers: {cmd:?}");
    run_image_command(cmd, "dpkg --triggers-only --pending")
}

/// A command run with the image as the root filesystem, in the environment
/// dpkg runs maintainer scripts in
fn image_command(
    ctx: &CompilerContext,
    program: &str,
    source_date_epoch: Option<u64>,
    env: &[(&str, &str)],
) -> Result<Command> {
    let mut isol = IsolationContext::builder(ctx.root_path());
    isol.ephemeral(false)
        .working_directory(Path::new("/"))
        .devtmpfs(Path::new("/dev"))
        .tmpfs(Path::new("/tmp"))
        .invocation_type(InvocationType::Pid2Pipe)
        .setenv(("TMPDIR", "/tmp"))
        .setenv((
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        ))
        .setenv(("DEBIAN_FRONTEND", "noninteractive"))
        // scripts that derive a charmap from the locale (console-setup's
        // setupcon) pick UTF-8, as they do on an installed system
        .setenv(("LANG", "C.UTF-8"))
        .setenv(("SYSTEMD_OFFLINE", "1"));
    for (key, value) in env {
        isol.setenv(((*key).to_owned(), (*value).to_owned()));
    }
    if let Some(epoch) = source_date_epoch {
        isol.setenv(("SOURCE_DATE_EPOCH".to_owned(), epoch.to_string()));
    }
    Ok(unshare(isol.build())?.command(program)?)
}

fn run_image_command(mut cmd: Command, what: &str) -> Result<()> {
    let out = cmd
        .output()
        .with_context(|| format!("while running {what}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "{what} failed ({}):\n{stdout}\n{stderr}",
            out.status
        ));
    }
    if !stdout.is_empty() || !stderr.is_empty() {
        debug!("{what}: {stdout}{stderr}");
    }
    Ok(())
}

fn bare_name(name: &str) -> &str {
    name.split(':').next().unwrap_or(name)
}

/// What dpkg orders packages by: their Depends and Pre-Depends (each clause
/// satisfied by any of its alternatives) and the virtual packages they
/// provide
#[derive(Debug, Default)]
struct Relations {
    depends: HashMap<String, Vec<Vec<String>>>,
    provides: HashMap<String, Vec<String>>,
    essential: HashSet<String>,
}

impl Relations {
    /// Parse package stanzas (a status file or control files), keyed by
    /// their Package field
    fn parse<'a>(stanzas: impl IntoIterator<Item = &'a str>) -> Self {
        let mut relations = Self::default();
        for stanza in stanzas {
            let fields = unfold(stanza);
            let field = |name: &str| {
                fields
                    .iter()
                    .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
                    .map(str::trim)
            };
            let Some(name) = field("Package") else {
                continue;
            };
            let clauses = ["Depends", "Pre-Depends"]
                .into_iter()
                .filter_map(field)
                .flat_map(parse_relation_field)
                .collect();
            for provided in field("Provides")
                .map(parse_relation_field)
                .unwrap_or_default()
            {
                for virtual_name in provided {
                    relations
                        .provides
                        .entry(virtual_name)
                        .or_default()
                        .push(name.to_owned());
                }
            }
            if field("Essential") == Some("yes") {
                relations.essential.insert(name.to_owned());
            }
            relations.depends.insert(name.to_owned(), clauses);
        }
        relations
    }

    /// The Essential packages among `names` and what they depend on, which
    /// is what apt installs for `apt-get install ?essential`
    fn essential_closure(&self, names: &[String]) -> HashSet<String> {
        let by_bare: HashMap<&str, &String> = names.iter().map(|n| (bare_name(n), n)).collect();
        let mut closure = HashSet::new();
        let mut stack: Vec<&str> = by_bare
            .keys()
            .copied()
            .filter(|n| self.essential.contains(*n))
            .collect();
        while let Some(name) = stack.pop() {
            let Some(full) = by_bare.get(name) else {
                continue;
            };
            if !closure.insert((*full).clone()) {
                continue;
            }
            for alt in self.depends.get(name).into_iter().flatten().flatten() {
                let providers = self.provides.get(alt.as_str()).into_iter().flatten();
                stack.extend(
                    std::iter::once(alt)
                        .chain(providers)
                        .map(String::as_str)
                        .filter(|n| by_bare.contains_key(n)),
                );
            }
        }
        closure
    }

    /// The first alternative of each clause, keyed and named as in `names`
    fn depends_names(&self, names: &[String]) -> HashMap<String, Vec<String>> {
        let by_bare: HashMap<&str, &String> = names.iter().map(|n| (bare_name(n), n)).collect();
        names
            .iter()
            .map(|name| {
                let deps = self
                    .depends
                    .get(bare_name(name))
                    .into_iter()
                    .flatten()
                    .filter_map(|clause| clause.first())
                    .map(|d| {
                        by_bare
                            .get(d.as_str())
                            .map_or_else(|| d.clone(), |n| (*n).clone())
                    })
                    .collect();
                (name.clone(), deps)
            })
            .collect()
    }
}

/// A stanza's lines with continuation lines folded into the field they
/// continue
fn unfold(stanza: &str) -> Vec<String> {
    let mut logical: Vec<String> = Vec::new();
    for line in stanza.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = logical.last_mut() {
                last.push_str(line);
            }
        } else {
            logical.push(line.to_owned());
        }
    }
    logical
}

/// Bare package names of each clause of a relation field:
/// "a (>= 1) | b:any, c" -> [["a", "b"], ["c"]]
fn parse_relation_field(field: &str) -> Vec<Vec<String>> {
    field
        .split(',')
        .map(|clause| {
            clause
                .split('|')
                .filter_map(|alt| {
                    let name = alt.split('(').next()?.split(':').next()?.trim();
                    (!name.is_empty()).then(|| name.to_owned())
                })
                .collect::<Vec<_>>()
        })
        .filter(|alts| !alts.is_empty())
        .collect()
}

/// Relations of the packages to install, from the control files the driver
/// extracted to `CONTROL_DIR`
fn control_relations(ctx: &CompilerContext, names: &[String]) -> Result<Relations> {
    let controls = names
        .iter()
        .map(|name| {
            let path = ctx.dst_path(format!("{CONTROL_DIR}/{name}/control"))?;
            std::fs::read_to_string(&path)
                .with_context(|| format!("while reading {}", path.display()))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Relations::parse(controls.iter().map(String::as_str)))
}

/// Relations of every package in the image's dpkg status db
fn status_relations(ctx: &CompilerContext) -> Result<Relations> {
    let path = ctx.dst_path("/var/lib/dpkg/status")?;
    let status = std::fs::read_to_string(&path)
        .with_context(|| format!("while reading {}", path.display()))?;
    Ok(Relations::parse(status.split("\n\n")))
}

/// `names` in the order apt gave (by bare name), any it did not name last
fn apt_order(names: &[String], order: &[String]) -> Vec<String> {
    let position: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, n)| (bare_name(n), i))
        .collect();
    let mut names = names.to_vec();
    names.sort_by_key(|n| position.get(bare_name(n)).copied().unwrap_or(usize::MAX));
    names
}

/// The bucket of dpkg's package table a package name falls in: FNV-1a of
/// the lowercased name, modulo the table's 65521 buckets (dpkg's
/// lib/dpkg/pkg-hash.c)
fn dpkg_hash_bin(name: &str) -> u32 {
    name.bytes()
        .map(|b| b.to_ascii_lowercase())
        .fold(2166136261u32, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(16777619)
        })
        % 65521
}

/// The order `dpkg --configure --pending` configures the unpacked `names`
/// in: the order its package table holds them in (by `dpkg_hash_bin`),
/// putting off each package whose dependencies are not configured yet until
/// the end of the queue, then, when a whole pass of the queue configures
/// nothing, settling for dependencies in a cycle with the package, and
/// finally for any that are unpacked.
fn dpkg_configure_order(names: &[String], relations: &Relations) -> Vec<String> {
    let pending: HashSet<&str> = names.iter().map(|n| bare_name(n)).collect();
    let mut queue: Vec<&String> = names.iter().collect();
    queue.sort_by_key(|n| dpkg_hash_bin(bare_name(n)));
    let mut queue: VecDeque<&String> = queue.into();

    let deps_of = |name: &str| relations.depends.get(name).into_iter().flatten();
    // whether `to` depends, through pending packages, on `from`
    let reaches = |from: &str, to: &str| {
        let mut seen = HashSet::new();
        let mut stack = vec![from];
        while let Some(name) = stack.pop() {
            if !seen.insert(name) {
                continue;
            }
            for alt in deps_of(name).flatten() {
                if alt == to {
                    return true;
                }
                if pending.contains(alt.as_str()) {
                    stack.push(alt);
                }
            }
        }
        false
    };

    let mut configured: HashSet<&str> = HashSet::new();
    let mut order = Vec::with_capacity(names.len());
    let mut dependtry = 1;
    let mut without_progress = 0;
    while let Some(name) = queue.pop_front() {
        let pkg = bare_name(name);
        let ready = |dep: &str| {
            configured.contains(dep)
                || (pending.contains(dep)
                    && (dependtry >= 3 || (dependtry == 2 && reaches(dep, pkg))))
        };
        let satisfied = deps_of(pkg).all(|clause| {
            clause.iter().any(|alt| {
                let providers = relations.provides.get(alt.as_str());
                if alt == pkg || ready(alt) {
                    return true;
                }
                if pending.contains(alt.as_str()) {
                    return false;
                }
                // installed already, or provided by a package that is
                relations.depends.contains_key(alt.as_str())
                    || providers.into_iter().flatten().any(|p| {
                        ready(p)
                            || (!pending.contains(p.as_str())
                                && relations.depends.contains_key(p.as_str()))
                    })
            })
        });
        // beyond that dpkg gives up on the package; it is configured anyway
        if satisfied || dependtry > 3 {
            configured.insert(pkg);
            order.push(name.clone());
            without_progress = 0;
        } else {
            queue.push_back(name);
            without_progress += 1;
            if without_progress > queue.len() {
                dependtry += 1;
                without_progress = 0;
            }
        }
    }
    order
}

/// Order package names the way dpkg would configure (dependencies first)
/// or remove (dependents first, when `reverse`) them. The resolver's
/// transaction order does not respect Depends, which breaks scripts that
/// rely on another package's side effects. Packages unrelated by Depends
/// keep their original relative order; dependency cycles (legal in dpkg)
/// deterministically fall back to original order for the packages
/// involved. Dependencies on packages outside `names` (already configured
/// in a parent layer, or not installed at all) are ignored.
fn sort_by_dependencies(
    ctx: &CompilerContext,
    names: Vec<String>,
    reverse: bool,
) -> Result<Vec<String>> {
    let deps = status_relations(ctx)?.depends_names(&names);
    Ok(topo_order(names, &deps, reverse))
}

fn topo_order(
    names: Vec<String>,
    deps: &HashMap<String, Vec<String>>,
    reverse: bool,
) -> Vec<String> {
    fn visit(
        name: &str,
        deps: &HashMap<String, Vec<String>>,
        in_set: &HashSet<&str>,
        ordered: &mut Vec<String>,
        done: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
    ) {
        if done.contains(name) || !visiting.insert(name.to_owned()) {
            // already ordered, or a dependency cycle: dpkg also just picks
            // an order, so keep the original one here
            return;
        }
        if let Some(ds) = deps.get(name) {
            for d in ds {
                if in_set.contains(d.as_str()) {
                    visit(d, deps, in_set, ordered, done, visiting);
                }
            }
        }
        visiting.remove(name);
        done.insert(name.to_owned());
        ordered.push(name.to_owned());
    }

    let in_set: HashSet<&str> = names.iter().map(String::as_str).collect();
    let mut ordered = Vec::with_capacity(names.len());
    let mut done = HashSet::new();
    let mut visiting = HashSet::new();
    for name in &names {
        visit(name, deps, &in_set, &mut ordered, &mut done, &mut visiting);
    }
    if reverse {
        ordered.reverse();
    }
    ordered
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
            unpack,
            configure,
        } = events.remove(0)
        {
            Ok((
                ResolvedTransaction {
                    install,
                    remove,
                    unpack,
                    configure,
                },
                dpkg_status,
            ))
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
    base_variant: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    dpkg_status: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    debs_dir: Option<&'a Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<DriverStage>,
}

/// What the driver does with the packages to install (see driver.py)
#[derive(Debug, Copy, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DriverStage {
    Control,
    Extract,
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
        install: Vec<InstallPackage>,
        remove: Vec<RemovePackage>,
        #[serde(default)]
        dpkg_status: Option<String>,
        #[serde(default)]
        unpack: Vec<String>,
        #[serde(default)]
        configure: Vec<String>,
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
        base_variant: String,
    },
}

impl DriverContext<'_> {
    pub fn plan(
        label: Label,
        build_appliance: PathBuf,
        archive_dir: PathBuf,
        target_arch: Arch,
        dpkg_status: Option<String>,
        base_variant: String,
    ) -> Self {
        Self::Plan {
            label,
            build_appliance,
            archive_dir,
            target_arch,
            dpkg_status,
            base_variant,
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

    fn base_variant(&self) -> &str {
        match self {
            Self::Plan { base_variant, .. } => base_variant,
            Self::Compile { .. } => "none",
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
    stage: Option<DriverStage>,
) -> Result<Vec<DriverEvent>> {
    // The archive dir is a symlinked_dir with relative symlinks that resolve
    // within the working directory tree. Mount the working directory and
    // reference the archive through it so symlinks resolve correctly.
    let cwd = std::env::current_dir()?;
    let archive_in_container = Path::new("/__antlir2__/working_directory").join(ctx.archive_dir());
    let debs_dir_in_container = ctx
        .debs_dir()
        .map(|d| Path::new("/__antlir2__/working_directory").join(d));

    // The driver only understands individual sonames; read the file here so
    // that neither it nor the resolver has to care where the list came from.
    let mut expanded_items = Vec::with_capacity(items.len());
    for item in items {
        match &item.apt {
            AptSource::SonamesSource(src) => {
                let contents = std::fs::read_to_string(src)
                    .with_context(|| format!("while reading {}", src.display()))?;
                for soname in contents.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    expanded_items.push(AptItem {
                        action: item.action,
                        apt: AptSource::Soname(soname.to_owned()),
                        feature_label: item.feature_label.clone(),
                        install_recommends: item.install_recommends,
                    });
                }
            }
            _ => expanded_items.push(item.clone()),
        }
    }
    let items = expanded_items.as_slice();

    let spec = DriverSpec {
        archive_dir: &archive_in_container,
        install_root: Path::new("/__antlir2__/root"),
        items,
        mode,
        arch: arch_to_deb_arch(ctx.target_arch()),
        resolved_transaction,
        base_variant: ctx.base_variant(),
        dpkg_status: ctx.dpkg_status(),
        debs_dir: debs_dir_in_container.as_deref(),
        stage,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relation_field() {
        assert_eq!(
            parse_relation_field(
                "dbus-bin (= 1.14.10-4ubuntu4), dbus-system-bus-common (>= 1.14.10), libc6 (>= 2.34)"
            ),
            vec![
                vec!["dbus-bin"],
                vec!["dbus-system-bus-common"],
                vec!["libc6"]
            ],
        );
        // every alternative is kept, arch qualifiers are stripped
        assert_eq!(
            parse_relation_field("default-dbus-session-bus | dbus-session-bus, libfoo:amd64"),
            vec![
                vec!["default-dbus-session-bus", "dbus-session-bus"],
                vec!["libfoo"]
            ],
        );
        assert!(parse_relation_field("").is_empty());
    }

    fn dep_map(pairs: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        pairs
            .iter()
            .map(|(name, ds)| (name.to_string(), ds.iter().map(|d| d.to_string()).collect()))
            .collect()
    }

    fn names(ns: &[&str]) -> Vec<String> {
        ns.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn test_topo_order_dependencies_first() {
        let deps = dep_map(&[
            ("dbus", &["dbus-system-bus-common", "libc6"]),
            ("dbus-system-bus-common", &[]),
        ]);
        // dbus listed first, but its dependency must configure first
        assert_eq!(
            topo_order(
                names(&["dbus", "dbus-system-bus-common", "tzdata"]),
                &deps,
                false
            ),
            names(&["dbus-system-bus-common", "dbus", "tzdata"]),
        );
        // unrelated packages keep their relative order
        assert_eq!(
            topo_order(
                names(&["tzdata", "dbus", "dbus-system-bus-common"]),
                &deps,
                false
            ),
            names(&["tzdata", "dbus-system-bus-common", "dbus"]),
        );
    }

    fn relations(stanzas: &str) -> Relations {
        Relations::parse(stanzas.split("\n\n"))
    }

    #[test]
    fn test_dpkg_configure_order_by_name_deferring_unmet() {
        let rel = relations(
            "Package: a\nDepends: c\n\nPackage: b\n\nPackage: c\nDepends: libc6 (>= 2.34)\n\nPackage: libc6\n",
        );
        // dpkg's table holds a, c, b in that order; a waits for c, and
        // libc6 is configured already
        assert_eq!(
            dpkg_configure_order(&names(&["c", "a", "b"]), &rel),
            names(&["c", "b", "a"]),
        );
    }

    #[test]
    fn test_dpkg_configure_order_alternatives_and_provides() {
        let rel = relations(
            "Package: a\nDepends: x | b\n\nPackage: b\nDepends: awk\n\nPackage: c\nDepends: y | b\n\nPackage: mawk\nProvides: awk\n\nPackage: y\n",
        );
        // c's clause is met by installed y at once, while a's is not by x
        // (not installed) and waits for b; b needs awk, which pending mawk
        // provides
        assert_eq!(
            dpkg_configure_order(&names(&["a", "b", "c", "mawk"]), &rel),
            names(&["c", "mawk", "b", "a"]),
        );
    }

    #[test]
    fn test_dpkg_configure_order_unsatisfiable() {
        let rel = relations("Package: a\nDepends: x\n\nPackage: b\n");
        // x is neither installed nor being installed: a is configured last
        assert_eq!(
            dpkg_configure_order(&names(&["a", "b"]), &rel),
            names(&["b", "a"]),
        );
    }

    #[test]
    fn test_dpkg_configure_order_breaks_cycles() {
        let rel = relations(
            "Package: a\nDepends: b\n\nPackage: b\nDepends: a\n\nPackage: c\nDepends: a\n",
        );
        // a whole pass defers all three; the cycle then breaks at b, the
        // first in the queue that a depends on
        assert_eq!(
            dpkg_configure_order(&names(&["a", "b", "c"]), &rel),
            names(&["b", "a", "c"]),
        );
    }

    #[test]
    fn test_dpkg_hash_bin() {
        // what dpkg's pkg_hash_find_set() computes: systemd's bucket comes
        // before udev's, whatever the case of the name
        assert_eq!(dpkg_hash_bin("systemd"), 22236);
        assert_eq!(dpkg_hash_bin("udev"), 41493);
        assert_eq!(dpkg_hash_bin("UDEV"), 41493);
    }

    #[test]
    fn test_apt_order() {
        assert_eq!(
            apt_order(
                &names(&["libc6:amd64", "base-files", "udev", "zz-unlisted"]),
                &names(&["udev", "libc6", "base-files"]),
            ),
            names(&["udev", "libc6:amd64", "base-files", "zz-unlisted"]),
            "names keep apt's order, matched without the arch qualifier, and unlisted ones go last",
        );
    }

    #[test]
    fn test_topo_order_reverse_removes_dependents_first() {
        let deps = dep_map(&[("dbus", &["dbus-system-bus-common"])]);
        assert_eq!(
            topo_order(names(&["dbus-system-bus-common", "dbus"]), &deps, true),
            names(&["dbus", "dbus-system-bus-common"]),
        );
    }

    #[test]
    fn test_topo_order_ignores_external_deps_and_cycles() {
        // libc6 is not in the transaction (comes from a parent layer)
        let deps = dep_map(&[("dbus", &["libc6", "dbus-system-bus-common"])]);
        assert_eq!(
            topo_order(names(&["dbus", "dbus-system-bus-common"]), &deps, false),
            names(&["dbus-system-bus-common", "dbus"]),
        );
        // cycles are legal in dpkg; the result is deterministic
        let deps = dep_map(&[("a", &["b"]), ("b", &["a"])]);
        assert_eq!(
            topo_order(names(&["a", "b"]), &deps, false),
            names(&["b", "a"]),
        );
    }
}
