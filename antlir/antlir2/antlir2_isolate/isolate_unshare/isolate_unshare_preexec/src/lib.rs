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
use std::os::fd::AsRawFd;
use std::path::PathBuf;

use nix::dir::Dir;
use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::mount::mount;
use nix::mount::MsFlags;
use nix::sched::unshare;
use nix::sched::CloneFlags;
use nix::sys::stat::Mode;
use nix::unistd::mkdir;
use nix::unistd::symlinkat;

static SCRATCH: &str = "/tmp/__antlir2__";
pub static NEWROOT: &str = "/tmp/__antlir2__/newroot";
static NEWROOT_PROC: &str = "/tmp/__antlir2__/newroot/proc";

/// MS_NOSYMFOLLOW (since Linux 5.10)
/// Do not follow symbolic links when resolving paths.  Symbolic links can still
/// be created, and readlink(1), readlink(2), realpath(1), and realpath(3) all
/// still work properly.
static MS_NOSYMFOLLOW: MsFlags = unsafe { MsFlags::from_bits_unchecked(256) };

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

    create_dir_all(SCRATCH)?;
    mount(
        None::<&str>,
        SCRATCH,
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
            MsFlags::MS_BIND | MsFlags::MS_REC | MsFlags::MS_RDONLY,
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
            Some("tmpfs"),
            MsFlags::empty(),
            None::<&str>,
        )?;
        if dev {
            let dir = Dir::open(tmpfs, OFlag::O_DIRECTORY, Mode::empty())?;
            symlinkat("/proc/self/fd", Some(dir.as_raw_fd()), "fd")?;
        }
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
            MsFlags::MS_BIND | MsFlags::MS_REC,
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
                // The target path could already exist but be a broken symlink
                // in which case this will return ENOENT, but since we use
                // MS_NOSYMFOLLOW the mount call might still succeed.
                ErrorKind::NotFound => Ok(()),
                // We get ROFS even if the file already exists, so maybe the
                // following mount still has a chance of working
                ErrorKind::ReadOnlyFilesystem => Ok(()),
                _ => Err(e),
            },
        }?;
        mount(
            Some(&bind.src),
            &bind.dst,
            None::<&str>,
            MsFlags::MS_BIND | MS_NOSYMFOLLOW,
            None::<&str>,
        )?;
    }
    // MS_BIND ignores MS_RDONLY, so let's go try to make all the readonly binds actually readonly
    // TODO(T185979228) we should also check for any recursive bind mounts
    // brought in by the first bind mount (since it necessarily has MS_REC)
    for bind in args.dir_binds.iter().chain(&args.file_binds) {
        if bind.ro {
            if let Err(e) = mount(
                Some("none"),
                &bind.dst,
                None::<&str>,
                MsFlags::MS_REMOUNT | MsFlags::MS_BIND | MsFlags::MS_REC | MsFlags::MS_RDONLY,
                None::<&str>,
            ) {
                if e != Errno::EPERM {
                    return Err(e.into());
                }
                // If we failed to make it readonly, carry on anyway. We can't
                // gain any new privileges here accidentally, so it's OK to
                // ignore the fact that some mounts like /mnt/gvfs cannot be
                // made readonly for whatever reason
            }
        }
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
