/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::ErrorKind;
use std::io::Seek;
use std::io::Write as _;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;

use antlir2_facts::RwDatabase;
use antlir2_facts::Transaction;
use antlir2_facts::fact::Fact;
use antlir2_facts::fact::deb::Deb;
use antlir2_facts::fact::dir_entry::DirEntry;
use antlir2_facts::fact::dir_entry::FileCommon;
use antlir2_facts::fact::dir_entry::Symlink;
use antlir2_facts::fact::rpm::Rpm;
use antlir2_facts::fact::user::Group;
use antlir2_facts::fact::user::User;
use antlir2_isolate::IsolationContext;
use antlir2_isolate::sys::unshare;
use antlir2_path::PathExt;
use antlir2_systemd::UnitFile;
use antlir2_users::group::EtcGroup;
use antlir2_users::passwd::EtcPasswd;
use anyhow::Context;
use anyhow::bail;
use anyhow::ensure;
use bon::builder;
use clap::Parser;
use fxhash::FxHashSet;
use tracing::warn;
use walkdir::WalkDir;

use crate::Error;
use crate::Result;
use crate::fact::subvolume::Subvolume;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    subvol_symlink: PathBuf,
    #[clap(long)]
    parent: Option<PathBuf>,
    #[clap(long)]
    build_appliance: Option<PathBuf>,
    #[clap(long)]
    db: PathBuf,
    #[clap(long)]
    rootless: bool,
}

fn populate(
    tx: &mut Transaction,
    root: &Path,
    build_appliance: Option<&Path>,
    package_manager: &str,
) -> anyhow::Result<()> {
    let root = root.canonicalize().context("while canonicalizing root")?;
    populate_files(tx, &root)?;
    populate_usergroups(tx, &root)?;
    if matches!(package_manager, "dnf" | "dnf5") {
        populate_rpms(tx, &root, build_appliance)?;
    } else if package_manager == "apt" {
        populate_debs(tx, &root)?;
    }
    populate_systemd_units(tx, &root)?;
    Ok(())
}

fn populate_files(tx: &mut Transaction, root: &Path) -> anyhow::Result<()> {
    let mut remove: FxHashSet<_> = tx
        .all_keys::<DirEntry>()?
        .chain(tx.all_keys::<Subvolume>()?)
        .collect();
    for entry in WalkDir::new(root) {
        let entry = entry?;
        let full_path = entry.path();
        let relpath = full_path
            .strip_prefix(root)
            .context("all paths must start with root dir")?;
        let path = Path::new("/").join(relpath);
        let meta = entry
            .metadata()
            .with_context(|| format!("while statting {}", full_path.display()))?;
        let common = FileCommon::new_with_metadata(path.clone(), &meta);
        let fact = if entry.file_type().is_dir() {
            DirEntry::Directory(common.into())
        } else if entry.file_type().is_symlink() {
            let raw_target = std::fs::read_link(full_path)
                .with_context(|| format!("while reading raw link {}", full_path.display()))?;
            DirEntry::Symlink(Symlink::new(common, raw_target))
        } else if entry.file_type().is_file() {
            DirEntry::RegularFile(common.into())
        } else {
            bail!(
                "{} was not a directory, symlink or file",
                full_path.display()
            );
        };
        remove.remove(&fact.key());
        tx.insert(&fact)?;
        // if this is a subvolume, log it so that antlir is aware that it's not just a directory
        if meta.ino() == antlir2_btrfs::INO_SUBVOL
            && antlir2_btrfs::Subvolume::open(full_path).is_ok()
        {
            tx.insert(&Subvolume::new(path))?;
        }
    }
    for remove in &remove {
        tx.delete::<DirEntry>(remove)?;
        tx.delete::<Subvolume>(remove)?;
    }
    Ok(())
}

fn populate_usergroups(tx: &mut Transaction, root: &Path) -> anyhow::Result<()> {
    let mut remove_users: FxHashSet<_> = tx.all_keys::<User>()?.collect();
    let mut remove_groups: FxHashSet<_> = tx.all_keys::<Group>()?.collect();
    let user_db: EtcPasswd = match std::fs::read_to_string(root.join("etc/passwd")) {
        Ok(contents) => contents.parse().context("while parsing /etc/passwd"),
        Err(e) => match e.kind() {
            ErrorKind::NotFound => Ok(Default::default()),
            _ => Err(anyhow::Error::from(e).context("while reading /etc/passwd")),
        },
    }?;
    for user in user_db.into_records() {
        let fact = User::new(user.name.clone(), user.uid.into());
        remove_users.remove(&fact.key());
        tx.insert(&fact)
            .with_context(|| format!("while inserting user '{}'", user.name))?;
    }
    let group_db: EtcGroup = match std::fs::read_to_string(root.join("etc/group")) {
        Ok(contents) => contents.parse().context("while parsing /etc/group"),
        Err(e) => match e.kind() {
            ErrorKind::NotFound => Ok(Default::default()),
            _ => Err(anyhow::Error::from(e).context("while reading /etc/group")),
        },
    }?;
    for group in group_db.into_records() {
        let fact = Group::new(group.name.clone(), group.gid.into(), group.users);
        remove_groups.remove(&fact.key());
        tx.insert(&fact)
            .with_context(|| format!("while inserting group '{}'", group.name))?;
    }
    for remove in &remove_users {
        tx.delete::<User>(remove)?;
    }
    for remove in &remove_groups {
        tx.delete::<Group>(remove)?;
    }
    Ok(())
}

const RPM_FACTS_SCRIPT: &str = include_str!("update_db/rpm_facts.py");

/// Interpreter used to run the rpm fact-finder inside `layer`.
///
/// CentOS/RHEL ship `/usr/libexec/platform-python` as a guaranteed-present
/// system interpreter and we keep using it there, since that is what those
/// images have always been exercised with. No other distro creates that path
/// (Fedora, Debian, ...), so fall back to resolving `python3` on $PATH inside
/// the layer -- the same way the `rpm` invocation above is resolved.
fn rpm_facts_interpreter(layer: &Path) -> &'static str {
    const PLATFORM_PYTHON: &str = "/usr/libexec/platform-python";
    // symlink_metadata, not exists(): platform-python is typically a symlink,
    // and exists() would resolve its target against the *host* root rather
    // than the layer.
    if layer.join_abs(PLATFORM_PYTHON).symlink_metadata().is_ok() {
        PLATFORM_PYTHON
    } else {
        "python3"
    }
}

fn populate_rpms(
    tx: &mut Transaction,
    root: &Path,
    build_appliance: Option<&Path>,
) -> anyhow::Result<()> {
    let mut remove: FxHashSet<_> = tx.all_keys::<Rpm>()?.collect();
    let mut list_cmd = match build_appliance {
        Some(build_appliance) => {
            let isol = unshare(
                IsolationContext::builder(build_appliance)
                    .ephemeral(true)
                    .inputs(("/__antlir2__/root", root))
                    .tmpfs_overlay(Path::new("/__antlir2__/root"))
                    .working_directory(Path::new("/"))
                    .build(),
            )
            .context("while preparing rpm unshare")?;
            let out = isol
                .command("rpm")?
                .arg("--root")
                .arg("/__antlir2__/root")
                .arg("-E")
                .arg("%{_dbpath}")
                .output()
                .context("while getting rpm db path")?;
            let out = std::str::from_utf8(&out.stdout).context("while decoding rpm db path")?;
            let dbpath = Path::new(out.trim());
            if !root.join_abs(dbpath).exists() {
                warn!("rpm db does not exist in image {}", root.display());
                return Ok(());
            }
            let mut cmd = isol.command(rpm_facts_interpreter(build_appliance))?;
            cmd.arg("-").arg("--installroot").arg("/__antlir2__/root");
            cmd
        }
        None => {
            let isol = unshare(
                IsolationContext::builder(root)
                    .ephemeral(true)
                    .working_directory(Path::new("/"))
                    .build(),
            )
            .context("while preparing rpm unshare")?;
            let mut cmd = isol.command(rpm_facts_interpreter(root))?;
            cmd.arg("-").arg("--installroot").arg("/");
            cmd
        }
    };
    let mut finder_script_mfd = memfd::MemfdOptions::default()
        .close_on_exec(false)
        .create("rpm_facts.py")?
        .into_file();
    finder_script_mfd.write_all(RPM_FACTS_SCRIPT.as_bytes())?;
    finder_script_mfd.rewind()?;
    list_cmd.stdin(finder_script_mfd);
    let out = list_cmd.output().context("while running fact-finder")?;
    ensure!(
        out.status.success(),
        "rpm fact-finder failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let facts: Vec<Rpm> = serde_json::from_slice(&out.stdout).with_context(|| {
        format!(
            "while parsing rpm facts: {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        )
    })?;

    for rpm in facts {
        remove.remove(&rpm.key());
        tx.insert(&rpm)
            .with_context(|| format!("while inserting rpm '{rpm}'"))?;
    }
    for remove in &remove {
        tx.delete::<Rpm>(remove)?;
    }
    Ok(())
}

fn populate_debs(tx: &mut Transaction, root: &Path) -> anyhow::Result<()> {
    let mut remove: FxHashSet<_> = tx.all_keys::<Deb>()?.collect();
    let status_path = root.join("var/lib/dpkg/status");
    let contents = match std::fs::read_to_string(&status_path) {
        Ok(c) => c,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            warn!(
                "dpkg status file does not exist in image {}",
                root.display()
            );
            return Ok(());
        }
        Err(e) => {
            return Err(anyhow::Error::from(e).context("while reading dpkg status file"));
        }
    };

    // Parse the dpkg status file. Stanzas are separated by blank lines.
    // Each field is "Key: Value", with continuation lines starting with a space.
    for stanza in contents.split("\n\n") {
        let stanza = stanza.trim();
        if stanza.is_empty() {
            continue;
        }

        let mut name = None;
        let mut version = None;
        let mut arch = None;
        let mut source = None;
        let mut installed_size = None;
        let mut status = None;

        for line in stanza.lines() {
            // Skip continuation lines (start with space/tab)
            if line.starts_with(' ') || line.starts_with('\t') {
                continue;
            }
            if let Some((key, value)) = line.split_once(": ") {
                match key {
                    "Package" => name = Some(value.to_owned()),
                    "Version" => version = Some(value.to_owned()),
                    "Architecture" => arch = Some(value.to_owned()),
                    "Source" => source = Some(value.to_owned()),
                    "Installed-Size" => installed_size = value.parse::<u64>().ok(),
                    "Status" => status = Some(value.to_owned()),
                    _ => {}
                }
            }
        }

        // Only include packages that are actually installed
        let is_installed = status
            .as_deref()
            .map(|s| s.contains("installed"))
            .unwrap_or(false);
        if !is_installed {
            continue;
        }

        let (Some(name), Some(version), Some(arch)) = (name, version, arch) else {
            continue;
        };

        let fact = Deb::builder()
            .name(name)
            .version(version)
            .arch(arch)
            .maybe_source(source)
            .maybe_installed_size(installed_size)
            .build();
        remove.remove(&fact.key());
        tx.insert(&fact)
            .with_context(|| format!("while inserting deb '{fact}'"))?;
    }

    for remove in &remove {
        tx.delete::<Deb>(remove)?;
    }
    Ok(())
}

fn populate_systemd_units(tx: &mut Transaction, root: &Path) -> anyhow::Result<()> {
    let mut remove: FxHashSet<_> = tx.all_keys::<UnitFile>()?.collect();
    for unit in
        antlir2_systemd::list_unit_files(root).context("while listing systemd unit files")?
    {
        remove.remove(&unit.key());
        tx.insert(&unit)
            .with_context(|| format!("while inserting unit {unit:?}"))?;
    }
    for remove in &remove {
        tx.delete::<UnitFile>(remove)?;
    }

    Ok(())
}

enum Root<'a> {
    Subvol(&'a Path),
}

impl Root<'_> {
    fn path(&self) -> &Path {
        match self {
            Self::Subvol(p) => p,
        }
    }
}

#[builder]
pub fn sync_db_with_layer(
    db: &Path,
    layer: &Path,
    build_appliance: Option<&Path>,
    package_manager: &str,
) -> Result<RwDatabase> {
    let mut db = RwDatabase::create(db)
        .with_context(|| format!("while preparing db {}", db.display()))
        .map_err(Error::Populate)?;

    let mut tx = db
        .transaction()
        .context("while preparing tx")
        .map_err(Error::Populate)?;

    let root = Root::Subvol(layer);

    populate(&mut tx, root.path(), build_appliance, package_manager).map_err(Error::Populate)?;

    tx.commit()
        .context("while committing tx")
        .map_err(Error::Populate)?;

    Ok(db)
}
