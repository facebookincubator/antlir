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
use antlir2_facts::Database;
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
    build_appliance: PathBuf,
    #[clap(long)]
    db: PathBuf,
    #[clap(long)]
    rootless: bool,
}

fn populate(db: &mut Database, root: &Path, build_appliance: &Path) -> Result<()> {
    let root = root.canonicalize().context("while canonicalizing root")?;
    populate_files(db, &root)?;
    populate_usergroups(db, &root)?;
    populate_rpms(db, &root, build_appliance)?;
    Ok(())
}

fn populate_files(db: &mut Database, root: &Path) -> Result<()> {
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

fn populate_usergroups(db: &mut Database, root: &Path) -> Result<()> {
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
    }
}

fn populate_rpms(db: &mut Database, root: &Path, build_appliance: &Path) -> Result<()> {
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
    let out = isol
        .command("rpm")?
        .arg("--root")
        .arg("/__antlir2__/root")
        .arg("-qa")
        .arg("--queryformat")
        .arg(OsStr::from_bytes(
            b"%{NAME}\xff%{EPOCH}\xff%{VERSION}\xff%{RELEASE}\xff%{ARCH}\xff%{CHANGELOGTEXT}\xff",
        ))
        .output()
        .context("while querying installed rpms")?;
    ensure!(
        out.status.success(),
        "rpm -qa failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    for (name, epoch, version, release, arch, changelog) in
        out.stdout.split(|b| *b == 0xff).tuples()
    {
        let name = decode_rpm_field!(name)?;
        let epoch = decode_rpm_field!(epoch)?;
        let version = decode_rpm_field!(version)?;
        let release = decode_rpm_field!(release)?;
        let arch = decode_rpm_field!(arch)?;
        let changelog = decode_rpm_field!(changelog)?;
        let rpm = Rpm::new(
            name,
            match epoch {
                "(none)" => 0,
                e => e
                    .parse()
                    .with_context(|| format!("while parsing epoch '{e}'"))?,
            },
            version,
            release,
            arch,
            match changelog {
                "(none)" => None,
                c => Some(c),
            },
        );
        db.insert(&rpm)
            .with_context(|| format!("while inserting rpm '{rpm}'"))?;
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
    let mut db =
        Database::open_readwrite(&args.db, rocksdb::Options::new().create_if_missing(true))
            .with_context(|| format!("while opening db {}", args.db.display()))?;

    let uid = nix::unistd::Uid::effective().as_raw();
    let gid = nix::unistd::Gid::effective().as_raw();

    let root_guard = rootless.map(|r| r.escalate()).transpose()?;
    populate(&mut db, &args.root, &args.build_appliance)?;

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
