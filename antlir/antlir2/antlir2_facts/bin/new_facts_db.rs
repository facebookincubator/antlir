/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;

use antlir2_facts::fact::dir_entry::DirEntry;
use antlir2_facts::fact::dir_entry::FileCommon;
use antlir2_facts::fact::dir_entry::Symlink;
use antlir2_facts::fact::rpm::Rpm;
use antlir2_facts::fact::user::Group;
use antlir2_facts::fact::user::User;
use antlir2_facts::fact::Fact;
use antlir2_facts::RwDatabase;
use antlir2_facts::Transaction;
use antlir2_isolate::sys::unshare;
use antlir2_isolate::IsolationContext;
use antlir2_overlayfs::OverlayFs;
use antlir2_systemd::UnitFile;
use antlir2_users::group::EtcGroup;
use antlir2_users::passwd::EtcPasswd;
use anyhow::bail;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use fxhash::FxHashSet;
use itertools::Itertools;
use json_arg::JsonFile;
use jwalk::WalkDir;
use tracing::warn;

#[derive(Parser)]
struct Args {
    #[clap(long, conflicts_with = "overlayfs")]
    subvol_symlink: Option<PathBuf>,
    #[clap(long, conflicts_with = "subvol_symlink")]
    overlayfs: Option<JsonFile<antlir2_overlayfs::BuckModel>>,
    #[clap(long)]
    parent: Option<PathBuf>,
    #[clap(long)]
    build_appliance: Option<PathBuf>,
    #[clap(long)]
    db: PathBuf,
    #[clap(long)]
    rootless: bool,
}

fn populate(tx: &mut Transaction, root: &Path, build_appliance: Option<&Path>) -> Result<()> {
    let root = root.canonicalize().context("while canonicalizing root")?;
    populate_files(tx, &root)?;
    populate_usergroups(tx, &root)?;
    populate_rpms(tx, &root, build_appliance)?;
    populate_systemd_units(tx, &root)?;
    Ok(())
}

fn populate_files(tx: &mut Transaction, root: &Path) -> Result<()> {
    let mut remove: FxHashSet<_> = tx.all_keys::<DirEntry>()?.collect();
    for entry in WalkDir::new(root).skip_hidden(false) {
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
            let raw_target = std::fs::read_link(&full_path)
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
    }
    for remove in &remove {
        tx.delete::<DirEntry>(remove)?;
    }
    Ok(())
}

fn populate_usergroups(tx: &mut Transaction, root: &Path) -> Result<()> {
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

macro_rules! decode_rpm_field {
    ($id:ident) => {
        std::str::from_utf8($id).context(stringify!(while decoding $id))
    };
    ($id:ident, opt) => {
        {
            let s = std::str::from_utf8($id).context(stringify!(while decoding $id))?;
            match s {
                "(none)" => Ok::<_, anyhow::Error>(None),
                _ => Ok(Some(s)),
            }
        }
    };
}

fn populate_rpms(tx: &mut Transaction, root: &Path, build_appliance: Option<&Path>) -> Result<()> {
    let mut remove: FxHashSet<_> = tx.all_keys::<Rpm>()?.collect();
    let mut list_cmd = match build_appliance {
        Some(build_appliance) => {
            let isol = unshare(
                IsolationContext::builder(build_appliance)
                    .ephemeral(false)
                    .inputs(("/__antlir2__/root", root))
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
            if !root
                .join(dbpath.strip_prefix("/").unwrap_or(dbpath))
                .exists()
            {
                warn!("rpm db does not exist in image {}", root.display());
                return Ok(());
            }
            let mut cmd = isol.command("rpm")?;
            cmd.arg("--root").arg("/__antlir2__/root");
            cmd
        }
        None => {
            let isol = unshare(
                IsolationContext::builder(root)
                    .ephemeral(false)
                    .working_directory(Path::new("/"))
                    .build(),
            )
            .context("while preparing rpm unshare")?;
            isol.command("rpm")?
        }
    };
    let out = list_cmd
        .arg("-qa")
        .arg("--queryformat")
        .arg(OsStr::from_bytes(
            b"%{NAME}\xff%{EPOCH}\xff%{VERSION}\xff%{RELEASE}\xff%{ARCH}\xff%{CHANGELOGTEXT}\xff%{OS}\xff%{SIZE}\xff%{SOURCERPM}\xff",
        ))
        .output();
    if matches!(out, Err(ref e) if e.kind() == ErrorKind::NotFound) {
        return Ok(Default::default());
    }
    let out = out.context("while querying installed rpms")?;
    ensure!(
        out.status.success(),
        "rpm -qa failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    for (name, epoch, version, release, arch, changelog, os, size, source_rpm) in
        out.stdout.split(|b| *b == 0xff).tuples()
    {
        let name = decode_rpm_field!(name)?;
        let epoch = decode_rpm_field!(epoch, opt)?;
        let version = decode_rpm_field!(version)?;
        let release = decode_rpm_field!(release)?;
        let arch = decode_rpm_field!(arch)?;
        let changelog = decode_rpm_field!(changelog, opt)?;
        let os = decode_rpm_field!(os, opt)?;
        let size = decode_rpm_field!(size, opt)?;
        let source_rpm = decode_rpm_field!(source_rpm)?;
        let rpm = Rpm::builder()
            .name(name)
            .epoch(match epoch {
                None => 0,
                Some(e) => e
                    .parse()
                    .with_context(|| format!("while parsing epoch '{e}'"))?,
            })
            .version(version)
            .release(release)
            .arch(arch)
            .changelog(changelog.map(|s| s.into()))
            .os(os.map(|s| s.into()))
            .size(size.map_or(Ok(0), |s| {
                s.parse()
                    .with_context(|| format!("while parsing size '{s}'"))
            })?)
            .source_rpm(source_rpm)
            .build();
        remove.remove(&rpm.key());
        tx.insert(&rpm)
            .with_context(|| format!("while inserting rpm '{rpm}'"))?;
    }
    for remove in &remove {
        tx.delete::<Rpm>(remove)?;
    }
    Ok(())
}

fn populate_systemd_units(tx: &mut Transaction, root: &Path) -> Result<()> {
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

enum Root {
    Subvol(PathBuf),
    Overlayfs(OverlayFs),
}

impl Root {
    fn path(&self) -> &Path {
        match self {
            Self::Subvol(p) => p,
            Self::Overlayfs(fs) => fs.mountpoint(),
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let rootless = if args.rootless {
        antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
        None
    } else {
        Some(antlir2_rootless::init().context("while dropping privileges")?)
    };

    ensure!(
        !args.db.exists(),
        "output '{}' already exists",
        args.db.display()
    );

    if let Some(parent) = &args.parent {
        std::fs::copy(parent, &args.db)
            .with_context(|| format!("while copying existing db '{}'", parent.display()))?;
    }

    let mut db = RwDatabase::create(&args.db)
        .with_context(|| format!("while creating db {}", args.db.display()))?;

    let mut tx = db.transaction().context("while preparing tx")?;

    let uid = nix::unistd::Uid::effective().as_raw();
    let gid = nix::unistd::Gid::effective().as_raw();

    let root_guard = rootless.map(|r| r.escalate()).transpose()?;

    antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;

    let root = match (args.subvol_symlink, args.overlayfs) {
        (Some(subvol_symlink), None) => Root::Subvol(subvol_symlink),
        (None, Some(model)) => Root::Overlayfs(
            // TODO: we ideally should be able to generate changes to the db
            // incrementally just by looking at the upper layer, but we still
            // need to run `rpm -q` (unless we want to record it as part of
            // feature.rpm and feature.chef_solo which seems very error-prone),
            // so just mount the overlayfs and treat it the same as a subvol
            OverlayFs::mount(
                antlir2_overlayfs::Opts::builder()
                    .model(model.into_inner())
                    .build(),
            )
            .context("while mounting overlayfs")?,
        ),
        _ => bail!("impossible combination"),
    };

    populate(&mut tx, root.path(), args.build_appliance.as_deref())?;

    // make sure all the output files are owned by the unprivileged user
    std::os::unix::fs::lchown(&args.db, Some(uid), Some(gid))
        .with_context(|| format!("while chowning {}", args.db.display()))?;
    drop(root_guard);

    tx.commit().context("while committing tx")?;

    Ok(())
}
