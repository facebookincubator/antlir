/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(io_error_more)]

use std::ffi::OsString;
use std::fs::create_dir_all;
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use cap_std::fs::Dir;
use clap::Parser;
use isolate_cfg::IsolationContext;
use json_arg::Json;
use nix::errno::Errno;
use nix::mount::mount;
use nix::mount::MsFlags;
use nix::sched::unshare;
use nix::sched::CloneFlags;
use nix::unistd::Gid;
use nix::unistd::Uid;
use nix::unistd::User;

/// MS_NOSYMFOLLOW (since Linux 5.10)
/// Do not follow symbolic links when resolving paths.  Symbolic links can still
/// be created, and readlink(1), readlink(2), realpath(1), and realpath(3) all
/// still work properly.
static MS_NOSYMFOLLOW: MsFlags = unsafe { MsFlags::from_bits_unchecked(256) };

#[derive(Debug, Parser)]
struct CliArgs {
    isolation: Json<IsolationContext<'static>>,
    program: OsString,
    #[clap(last = true)]
    program_args: Vec<OsString>,
}

fn main() {
    if let Err(e) = do_main() {
        let e = format!("{e:#?}");
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn do_main() -> Result<()> {
    let args = CliArgs::parse();
    isolate_unshare_preexec(&args.isolation).context("while setting up in-process isolation")?;
    let mut cmd = Command::new(args.program);
    cmd.env_clear();
    // reasonable default PATH (same as systemd-nspawn uses)
    cmd.env(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    cmd.env("container", "antlir2");
    cmd.env("USER", args.isolation.user.as_ref());
    if let Some(term) = std::env::var_os("TERM") {
        cmd.env("TERM", term);
    }
    for (key, val) in &args.isolation.setenv {
        cmd.env(key, val);
    }
    let err = cmd.args(args.program_args).exec();
    Err(Error::from(err).context("failed to exec child"))
}

trait DirExt {
    fn abspath(&self) -> PathBuf;
}

impl DirExt for Dir {
    fn abspath(&self) -> PathBuf {
        std::fs::read_link(format!("/proc/self/fd/{}", self.as_raw_fd()))
            .expect("failed to read /proc/self/fd to find open path")
    }
}

impl DirExt for cap_std::fs::File {
    fn abspath(&self) -> PathBuf {
        std::fs::read_link(format!("/proc/self/fd/{}", self.as_raw_fd()))
            .expect("failed to read /proc/self/fd to find open path")
    }
}

trait PathExt {
    fn strip_abs(&self) -> &Path;
}

impl PathExt for Path {
    fn strip_abs(&self) -> &Path {
        self.strip_prefix("/").unwrap_or(self)
    }
}

impl PathExt for PathBuf {
    fn strip_abs(&self) -> &Path {
        self.strip_prefix("/").unwrap_or(self)
    }
}

#[deny(unused_variables)]
fn isolate_unshare_preexec(isol: &IsolationContext) -> Result<()> {
    let IsolationContext {
        layer,
        working_directory,
        setenv: _,
        platform,
        inputs,
        outputs,
        user,
        ephemeral,
        tmpfs,
        devtmpfs,
        tmpfs_overlay,
        hostname,
        readonly,
        // isolate_unshare crate already ensures that these are not configured
        invocation_type: _,
        register: _,
        enable_network: _,
    } = &isol;

    unshare(CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWNET | CloneFlags::CLONE_NEWUTS)
        .context("while unsharing into new namespaces")?;
    // Remount / as private so that we don't let any changes escape back to the
    // parent mount namespace.
    // This is basically equivalent to `mount --make-rprivate /`
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    )
    .context("while making / private")?;

    let scratch = Path::new("/tmp/__antlir2__");
    create_dir_all(scratch)
        .with_context(|| format!("while making scratch dir '{}'", scratch.display()))?;
    mount(
        None::<&str>,
        scratch,
        Some("tmpfs"),
        MsFlags::empty(),
        None::<&str>,
    )
    .with_context(|| {
        format!(
            "while mounting tmpfs on scratch dir '{}'",
            scratch.display()
        )
    })?;
    let scratch = Dir::open_ambient_dir(scratch, cap_std::ambient_authority())
        .with_context(|| format!("while opening scratch dir '{}", scratch.display()))?;

    scratch
        .create_dir("newroot")
        .context("while making newroot")?;
    let newroot = scratch
        .open_dir("newroot")
        .context("while opening newroot")?;

    if *ephemeral {
        scratch
            .create_dir("ephemeral_upper")
            .context("mkdir ephemeral_upper")?;
        scratch
            .create_dir("ephemeral_work")
            .context("mkdir ephemeral_work")?;
        scratch
            .create_dir("ephemeral_lower")
            .context("mkdir ephemeral_lower")?;
        let scratch_abspath = scratch.abspath();
        mount(
            Some(layer.as_ref()),
            &scratch_abspath.join("ephemeral_lower"),
            None::<&str>,
            MsFlags::MS_BIND | MsFlags::MS_REC | MsFlags::MS_RDONLY,
            None::<&str>,
        )
        .context("while mounting root at ephemeral lower")?;
        let mut mount_opts = OsString::from("lowerdir=");
        mount_opts.push(scratch_abspath.join("ephemeral_lower").into_os_string());
        mount_opts.push(",upperdir=");
        mount_opts.push(scratch_abspath.join("ephemeral_upper").into_os_string());
        mount_opts.push(",workdir=");
        mount_opts.push(scratch_abspath.join("ephemeral_work").into_os_string());
        mount(
            Some("overlay"),
            &newroot.abspath(),
            Some("overlay"),
            MsFlags::empty(),
            Some(mount_opts.as_os_str()),
        )
        .context("while mounting ephemeral overlayfs root")?;
    } else {
        let mut root_flags = MsFlags::MS_BIND | MsFlags::MS_REC;
        if *readonly {
            root_flags |= MsFlags::MS_RDONLY;
        }

        mount(
            Some(layer.as_ref()),
            &newroot.abspath(),
            None::<&str>,
            root_flags,
            None::<&str>,
        )
        .context("while mounting root layer")?;
    }

    let newroot = scratch
        .open_dir("newroot")
        .context("while (re)opening newroot")?;

    for (tmpfs, dev) in tmpfs
        .iter()
        .map(|t| (t, false))
        .chain(devtmpfs.iter().map(|t| (t, true)))
    {
        match newroot.create_dir_all(tmpfs.strip_abs()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e),
        }
        .with_context(|| format!("while making tmpfs mountpoint at '{}'", tmpfs.display()))?;
        nix::mount::mount(
            None::<&str>,
            &newroot.abspath().join(tmpfs.strip_abs()),
            Some("tmpfs"),
            MsFlags::empty(),
            if dev { Some("mode=0755") } else { None },
        )
        .with_context(|| format!("while mounting tmpfs at '{}'", tmpfs.display()))?;
        let tmpfs = newroot
            .open_dir(tmpfs.strip_abs())
            .with_context(|| format!("while opening tmpfs '{}'", tmpfs.display()))?;
        if dev {
            tmpfs
                .symlink_contents("/proc/self/fd", "fd")
                .context("while creating /dev/fd symlink")?;

            for devname in ["null", "random", "urandom"] {
                let dev = tmpfs
                    .create(devname)
                    .with_context(|| format!("while creating device file '{devname}'"))?;
                nix::mount::mount(
                    Some(&Path::new("/dev").join(devname)),
                    &dev.abspath(),
                    None::<&str>,
                    MsFlags::MS_BIND | MS_NOSYMFOLLOW,
                    None::<&str>,
                )
                .with_context(|| format!("while mounting device node '{devname}'"))?;
            }
        }
    }

    for (dst, src, ro) in inputs
        .iter()
        .map(|(dst, src)| (dst, src, true))
        .chain(platform.iter().map(|(dst, src)| (dst, src, true)))
        .chain(outputs.iter().map(|(dst, src)| (dst, src, false)))
    {
        let ft = src
            .metadata()
            .with_context(|| format!("while statting '{}'", src.display()))?
            .file_type();
        let dst = if let Ok(target) = std::fs::read_link(layer.join(dst.strip_abs())) {
            dst.parent().unwrap_or(dst).join(target)
        } else {
            dst.clone().into_owned()
        };
        if ft.is_dir() {
            match newroot.create_dir_all(dst.strip_abs()) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
                Err(e) => Err(e),
            }
            .with_context(|| format!("while creating mountpoint '{}'", dst.display()))?;
            let dst = newroot.open_dir(dst.strip_abs())?;
            mount(
                Some(src.as_ref()),
                &dst.abspath(),
                None::<&str>,
                MsFlags::MS_BIND | MsFlags::MS_REC,
                None::<&str>,
            )
            .with_context(|| format!("while mounting '{}'", dst.abspath().display()))?;
        } else {
            if let Some(parent) = dst.parent() {
                match newroot.create_dir_all(parent.strip_abs()) {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
                    Err(e) => Err(e),
                }
                .with_context(|| format!("while creating parent dir '{}'", parent.display()))?;
            }

            match newroot.open_with(
                dst.strip_abs(),
                cap_std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true),
            ) {
                Ok(_) => Ok(()),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
                // The target path could already exist but be a broken symlink
                // in which case this will return ENOENT, but since we use
                // MS_NOSYMFOLLOW the mount call might still succeed.
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                // We get ROFS even if the file already exists, so maybe the
                // following mount still has a chance of working
                Err(e) if e.kind() == ErrorKind::ReadOnlyFilesystem => Ok(()),
                Err(e) => Err(e),
            }
            .with_context(|| {
                format!("while creating bind mount dst file for '{}'", dst.display())
            })?;
            let dst = newroot.open(dst.strip_abs())?;
            mount(
                Some(src.as_ref()),
                &dst.abspath(),
                None::<&str>,
                MsFlags::MS_BIND | MS_NOSYMFOLLOW,
                None::<&str>,
            )
            .with_context(|| {
                if !src.exists() {
                    format!(
                        "bind src '{}' for '{}' does not exist",
                        src.display(),
                        dst.abspath().display()
                    )
                } else {
                    format!(
                        "while mounting {} on {}",
                        src.display(),
                        dst.abspath().display()
                    )
                }
            })?;
        }

        // MS_BIND ignores MS_RDONLY, so let's go try to make all the readonly
        // binds actually readonly.
        // TODO(T185979228) we should also check for any
        // recursive bind mounts brought in by the first bind mount (since it
        // necessarily has MS_REC)
        if ro {
            let dst = newroot.abspath().join(dst.strip_abs());
            match mount(
                Some("none"),
                &dst,
                None::<&str>,
                MsFlags::MS_REMOUNT | MsFlags::MS_BIND | MsFlags::MS_REC | MsFlags::MS_RDONLY,
                None::<&str>,
            ) {
                Ok(()) => Ok(()),
                Err(Errno::EPERM) => {
                    // If we failed to make it readonly, carry on anyway. We
                    // can't gain any new privileges here accidentally, so it's
                    // OK to ignore the fact that some mounts like /mnt/gvfs
                    // cannot be made readonly for whatever reason
                    Ok(())
                }
                Err(e) => Err(std::io::Error::from(e)),
            }
            .with_context(|| format!("while making mount '{}' readonly", dst.display()))?;
        }
    }

    if !tmpfs_overlay.is_empty() {
        scratch.create_dir("tmpfs_overlay")?;
        let overlay_root = scratch.open_dir("tmpfs_overlay")?;
        for (idx, path) in tmpfs_overlay.iter().enumerate() {
            let dst = newroot.open_dir(path.strip_abs())?;
            overlay_root.create_dir(format!("upper_{idx}"))?;
            overlay_root.create_dir(format!("work_{idx}"))?;
            let upper = overlay_root.open_dir(format!("upper_{idx}"))?;
            let work = overlay_root.open_dir(format!("work_{idx}"))?;
            let mut opts = OsString::from("lowerdir=");
            opts.push(dst.abspath());
            opts.push(",upperdir=");
            opts.push(upper.abspath());
            opts.push(",workdir=");
            opts.push(work.abspath());
            mount(
                Some("overlay"),
                &dst.abspath(),
                Some("overlay"),
                MsFlags::empty(),
                Some(opts.as_os_str()),
            )
            .with_context(|| format!("while mounting tmpfs overlay at '{}'", path.display()))?;
        }
    }

    if let Some(hostname) = hostname {
        nix::unistd::sethostname(hostname.as_ref()).context("while setting hostname")?;
    }

    match newroot.create_dir("proc") {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }?;
    // TODO: when we support CLONE_NEWPID, this should be a fresh mount of /proc
    // instead of binding it from the parent ns
    nix::mount::mount(
        Some("/proc"),
        &newroot.open_dir("proc")?.abspath(),
        Some("proc"),
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>,
    )
    .context("while mounting /proc")?;

    nix::unistd::chroot(&newroot.abspath())?;
    if let Some(wd) = working_directory {
        std::env::set_current_dir(wd)
            .with_context(|| format!("while changing directory into '{}'", wd.display()))?;
    }

    let (uid, gid) = if user == "root" {
        (Uid::from_raw(0), Gid::from_raw(0))
    } else {
        let user = User::from_name(user)
            .with_context(|| format!("while looking up user '{user}'"))?
            .with_context(|| format!("no such user '{user}'"))?;
        (user.uid, user.gid)
    };

    nix::unistd::setgid(gid)?;
    nix::unistd::setuid(uid)?;

    Ok(())
}
