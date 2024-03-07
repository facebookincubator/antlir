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
use antlir2_facts::RwDatabase;
use antlir2_isolate::sys::unshare;
use antlir2_isolate::IsolationContext;
use antlir2_users::group::EtcGroup;
use antlir2_users::passwd::EtcPasswd;
use anyhow::bail;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use itertools::Itertools;
use jwalk::WalkDir;
use tracing::trace;
use tracing::warn;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    root: PathBuf,
    #[clap(long)]
    build_appliance: Option<PathBuf>,
    #[clap(long)]
    db: PathBuf,
    #[clap(long)]
    rootless: bool,
}

fn populate(db: &mut RwDatabase, root: &Path, build_appliance: Option<&Path>) -> Result<()> {
    let root = root.canonicalize().context("while canonicalizing root")?;
    populate_files(db, &root)?;
    populate_usergroups(db, &root)?;
    populate_rpms(db, &root, build_appliance)?;
    Ok(())
}

fn populate_files(db: &mut RwDatabase, root: &Path) -> Result<()> {
    for entry in WalkDir::new(root) {
        let entry = entry?;
        let full_path = entry.path();
        let relpath = full_path
            .strip_prefix(root)
            .context("all paths must start with root dir")?;
        let path = Path::new("/").join(relpath);
        trace!("adding {path:?}");
        let meta = entry
            .metadata()
            .with_context(|| format!("while statting {}", full_path.display()))?;
        let common = FileCommon::new_with_metadata(path.clone(), &meta);
        if entry.file_type().is_dir() {
            db.insert(&DirEntry::Directory(common.into()))?;
        } else if entry.file_type().is_symlink() {
            let raw_target = std::fs::read_link(&full_path)
                .with_context(|| format!("while reading raw link {}", full_path.display()))?;
            db.insert(&DirEntry::Symlink(Symlink::new(common, raw_target)))?;
        } else if entry.file_type().is_file() {
            db.insert(&DirEntry::RegularFile(common.into()))?;
        } else {
            bail!(
                "{} was not a directory, symlink or file",
                full_path.display()
            );
        }
    }
    Ok(())
}

fn populate_usergroups(db: &mut RwDatabase, root: &Path) -> Result<()> {
    let user_db: EtcPasswd = match std::fs::read_to_string(root.join("etc/passwd")) {
        Ok(contents) => contents.parse().context("while parsing /etc/passwd"),
        Err(e) => match e.kind() {
            ErrorKind::NotFound => Ok(Default::default()),
            _ => Err(anyhow::Error::from(e).context("while reading /etc/passwd")),
        },
    }?;
    for user in user_db.into_records() {
        db.insert(&User::new(user.name.clone(), user.uid.into()))
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
        db.insert(&Group::new(
            group.name.clone(),
            group.gid.into(),
            group.users,
        ))
        .with_context(|| format!("while inserting group '{}'", group.name))?;
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

fn populate_rpms(db: &mut RwDatabase, root: &Path, build_appliance: Option<&Path>) -> Result<()> {
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
        db.insert(&rpm)
            .with_context(|| format!("while inserting rpm '{rpm}'"))?;
    }

    for unit in
        antlir2_systemd::list_unit_files(root).context("while listing systemd unit files")?
    {
        db.insert(&unit)
            .with_context(|| format!("while inserting unit {unit:?}"))?;
    }

    Ok(())
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

    if args.db.exists() {
        bail!(
            "{} already exists - populate currently only works with completely new dbs",
            args.db.display()
        );
    }
    let mut db = RwDatabase::open(&args.db, rocksdb::Options::new().create_if_missing(true))
        .with_context(|| format!("while opening db {}", args.db.display()))?;

    let uid = nix::unistd::Uid::effective().as_raw();
    let gid = nix::unistd::Gid::effective().as_raw();

    let root_guard = rootless.map(|r| r.escalate()).transpose()?;
    populate(&mut db, &args.root, args.build_appliance.as_deref())?;

    // make sure all the output files are owned by the unprivileged user
    for entry in jwalk::WalkDir::new(&args.db) {
        let entry = entry?;
        let path = entry.path();
        std::os::unix::fs::lchown(&path, Some(uid), Some(gid))
            .with_context(|| format!("while chowning {}", path.display()))?;
    }
    drop(root_guard);

    Ok(())
}
