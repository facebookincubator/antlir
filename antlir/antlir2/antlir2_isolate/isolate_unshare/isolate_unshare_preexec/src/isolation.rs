/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::fs::create_dir_all;
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::path::PathBuf;

use antlir2_path::PathExt;
use anyhow::Context;
use anyhow::Result;
use cap_std::fs::Dir;
use cap_std::fs::OpenOptionsExt as _;
use isolate_cfg::IsolationContext;
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

#[cfg_attr(facebook, deny(unused_variables))]
pub(crate) fn setup_isolation(isol: &IsolationContext) -> Result<()> {
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
        enable_network,
    } = isol;

    let mut clone_flags = CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWUTS;
    if !enable_network {
        clone_flags |= CloneFlags::CLONE_NEWNET;
    }

    unshare(clone_flags).context("while unsharing into new namespaces")?;

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

    // Ensure that the loopback interface is up in our new network namespace in
    // case anything wants to bind to it for whatever reason
    crate::net::bring_loopback_up().context("while bringing up loopback interface")?;

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
            &newroot.abspath().join_abs(tmpfs),
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

            // Things like `sem_open` requires a usable `/dev/shm`.
            tmpfs
                .create_dir("shm")
                .context("while creating directory 'shm'")?;
            let dir = tmpfs
                .open_dir("shm")
                .context("while opening shm mountpoint")?
                .into_std_file();
            nix::mount::mount(
                None::<&str>,
                &dir.abspath(),
                Some("tmpfs"),
                MsFlags::empty(),
                None::<&str>,
            )
            .context("while mounting shm")?;
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
        let dst = if let Ok(target) = std::fs::read_link(layer.join_abs(dst)) {
            dst.parent().unwrap_or(dst).join(target)
        } else {
            dst.clone().into_owned()
        };
        let dst = if ft.is_dir() {
            match newroot.create_dir_all(dst.strip_abs()) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
                Err(e) => Err(e),
            }
            .with_context(|| format!("while creating mountpoint '{}'", dst.display()))?;
            newroot
                .open_dir(dst.strip_abs())
                .context("while opening mountpoint")?
                .into_std_file()
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
                    .write(true)
                    .custom_flags(libc::O_NOFOLLOW),
            ) {
                Ok(f) => Ok(f),
                // We get ROFS if the file already exist, so just open it
                Err(e) if e.kind() == ErrorKind::ReadOnlyFilesystem => {
                    newroot.open(dst.strip_abs())
                }
                Err(e) => Err(e),
            }
            .with_context(|| format!("while creating bind mount dst file for '{}'", dst.display()))?
            .into_std()
        };
        mount(
            Some(src.as_ref()),
            &dst.abspath(),
            None::<&str>,
            MsFlags::MS_BIND | MsFlags::MS_REC | MS_NOSYMFOLLOW,
            None::<&str>,
        )
        .with_context(|| format!("while mounting '{}'", dst.abspath().display()))?;

        // MS_BIND ignores MS_RDONLY, so use the new mount api to make it
        // actually readonly.
        #[cfg(facebook)]
        if ro {
            let dst_mountpoint = dst.abspath();
            crate::new_mount_api::make_mount_readonly(&dst_mountpoint)
                .with_context(|| format!("while making '{}' readonly", dst_mountpoint.display()))?;
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
    nix::mount::mount(
        None::<&str>,
        &newroot.open_dir("proc")?.abspath(),
        Some("proc"),
        MsFlags::empty(),
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

impl DirExt for std::fs::File {
    fn abspath(&self) -> PathBuf {
        std::fs::read_link(format!("/proc/self/fd/{}", self.as_raw_fd()))
            .expect("failed to read /proc/self/fd to find open path")
    }
}
