/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// See safety notes about pre_exec here
// https://doc.rust-lang.org/stable/std/os/unix/process/trait.CommandExt.html#tymethod.pre_exec
//
// TL;DR we basically can't do anything but pure-logic here - even `malloc` can
// easily fail catastrophically.
//
// This isn't really possible to enforce at compile time, so the best we can
// really do is move anything running in this post-fork-pre-exec context into
// this separate crate which we can look at more carefully to ensure that it's
// not doing anything unsafe.
//
// Using #[no_std] would help encourage this, but it's not really feasible to
// pass owned data into this implementation without using std types like
// PathBuf.

#![feature(io_error_more)]

use std::fs::create_dir;
use std::fs::create_dir_all;
use std::fs::File;
use std::io::ErrorKind;
use std::io::Result;
use std::path::PathBuf;

use nix::errno::Errno;
use nix::mount::mount;
use nix::mount::MsFlags;
use nix::sched::unshare;
use nix::sched::CloneFlags;
use nix::sys::stat::Mode;
use nix::unistd::mkdir;

pub static NEWROOT: &str = "/tmp/__antlir2__/newroot";
static NEWROOT_PROC: &str = "/tmp/__antlir2__/newroot/proc";

pub struct Args {
    pub root: PathBuf,
    pub root_ro: bool,
    pub dir_binds: Vec<Bind>,
    pub file_binds: Vec<Bind>,
    pub tmpfs: Vec<PathBuf>,
    pub devtmpfs: Vec<PathBuf>,
    pub working_dir: PathBuf,
    pub hostname: Option<String>,
    pub uid: u32,
    pub gid: u32,
    pub ephemeral: bool,
}

pub struct Bind {
    pub src: PathBuf,
    pub dst: PathBuf,
    pub ro: bool,
}

pub fn isolate_unshare_preexec(args: &Args) -> Result<()> {
    unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWNET | CloneFlags::CLONE_NEWUTS)?;
    // Remount / as private so that we don't let any changes escape back to the
    // parent mount namespace.
    // This is basically equivalent to `mount --make-rprivate /`
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    )?;

    create_dir_all("/tmp/__antlir2__")?;
    mount(
        None::<&str>,
        "/tmp/__antlir2__",
        Some("tmpfs"),
        MsFlags::empty(),
        None::<&str>,
    )?;
    create_dir_all(NEWROOT)?;

    if args.ephemeral {
        create_dir("/tmp/__antlir2__/ephemeral_upper")?;
        create_dir("/tmp/__antlir2__/ephemeral_work")?;
        create_dir("/tmp/__antlir2__/ephemeral_lower")?;
        mount(
            Some(&args.root),
            "/tmp/__antlir2__/ephemeral_lower",
            None::<&str>,
            MsFlags::MS_BIND | MsFlags::MS_REC,
            None::<&str>,
        )?;
        mount(
            Some("overlay"),
            NEWROOT,
            Some("overlay"),
            MsFlags::empty(),
            Some(
                "lowerdir=/tmp/__antlir2__/ephemeral_lower,upperdir=/tmp/__antlir2__/ephemeral_upper,workdir=/tmp/__antlir2__/ephemeral_work",
            ),
        )?;
    } else {
        let mut root_flags = MsFlags::MS_BIND | MsFlags::MS_REC;
        if args.root_ro {
            root_flags |= MsFlags::MS_RDONLY;
        }

        mount(
            Some(&args.root),
            NEWROOT,
            None::<&str>,
            root_flags,
            None::<&str>,
        )?;
    }

    for (tmpfs, dev) in args
        .tmpfs
        .iter()
        .map(|t| (t, false))
        .chain(args.devtmpfs.iter().map(|t| (t, true)))
    {
        match mkdir(tmpfs, Mode::S_IRWXU) {
            Ok(()) => Ok(()),
            Err(Errno::EEXIST) => Ok(()),
            Err(e) => Err(e),
        }?;
        nix::mount::mount(
            None::<&str>,
            tmpfs,
            if dev { Some("devtmpfs") } else { Some("tmpfs") },
            MsFlags::empty(),
            None::<&str>,
        )?;
    }

    for bind in &args.dir_binds {
        match create_dir_all(&bind.dst) {
            Ok(()) => Ok(()),
            Err(e) => match e.kind() {
                ErrorKind::AlreadyExists => Ok(()),
                _ => Err(e),
            },
        }?;
        mount(
            Some(&bind.src),
            &bind.dst,
            None::<&str>,
            MsFlags::MS_BIND
                | MsFlags::MS_REC
                | (if bind.ro {
                    MsFlags::MS_RDONLY
                } else {
                    MsFlags::empty()
                }),
            None::<&str>,
        )?;
    }
    for bind in &args.file_binds {
        if let Some(parent) = bind.dst.parent() {
            match create_dir_all(parent) {
                Ok(()) => Ok(()),
                Err(e) => match e.kind() {
                    ErrorKind::AlreadyExists => Ok(()),
                    _ => Err(e),
                },
            }?;
        }

        match File::create(&bind.dst) {
            Ok(_) => Ok(()),
            Err(e) => match e.kind() {
                ErrorKind::AlreadyExists => Ok(()),
                // we get ROFS even if the file already exists, so maybe the
                // following mount still has a chance of working
                ErrorKind::ReadOnlyFilesystem => Ok(()),
                _ => Err(e),
            },
        }?;
        mount(
            Some(&bind.src),
            &bind.dst,
            None::<&str>,
            MsFlags::MS_BIND
                | (if bind.ro {
                    MsFlags::MS_RDONLY
                } else {
                    MsFlags::empty()
                }),
            None::<&str>,
        )?;
    }

    if let Some(hostname) = &args.hostname {
        nix::unistd::sethostname(hostname)?;
    }

    match create_dir(NEWROOT_PROC) {
        Ok(()) => Ok(()),
        Err(e) => match e.kind() {
            ErrorKind::AlreadyExists => Ok(()),
            _ => Err(e),
        },
    }?;
    // TODO: when we support CLONE_NEWPID, this should be a fresh mount of /proc
    // instead of binding it from the parent ns
    nix::mount::mount(
        Some("/proc"),
        NEWROOT_PROC,
        Some("proc"),
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>,
    )?;

    nix::unistd::chroot(NEWROOT)?;
    std::env::set_current_dir(&args.working_dir)?;

    nix::unistd::setgid(args.gid.into())?;
    nix::unistd::setuid(args.uid.into())?;

    Ok(())
}
