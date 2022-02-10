/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/// At first glance, this looks like reinventing the wheels that busybox and
/// uroot have already invented. However, this is actually well-reasoned: systemd
/// calls /usr/bin/mount to mount filesystems from mount units - unless we fix
/// systemd to use the syscall, we have to provide _something_ to implement
/// /usr/bin/mount. We could use busybox/uroot, but since `mount` is the only
/// tool that we need in the base initrd (we need a full userspace in the debug
/// initrd), just implement mount with a thin wrapper around the syscall to save
/// 1.2M by not including busybox busybox (or even more for uroot).
/// This is definitely not an exhaustive implementation of everything
/// /usr/bin/mount usually does, but is enough to cover all the calls in the
/// initrd environment.
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use nix::mount::MsFlags;
use slog::Logger;
use std::fs::File;
use std::io::{BufRead, BufReader};
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Opts {
    pub source: String,
    pub target: PathBuf,
    #[structopt(long = "bind")]
    pub bind: bool,
    #[structopt(short = "t")]
    pub fstype: Option<String>,
    #[structopt(short, require_delimiter(true))]
    pub options: Vec<String>,
}

/// parse options given with -o into data and flags
fn parse_options(options: Vec<String>) -> (Vec<String>, MsFlags) {
    let mut flags = MsFlags::empty();
    let mut data = Vec::new();
    for opt in options {
        match opt.as_str() {
            "ro" => flags.insert(MsFlags::MS_RDONLY),
            "remount" => flags.insert(MsFlags::MS_REMOUNT),
            "rw" => flags.remove(MsFlags::MS_RDONLY),
            "relatime" => flags.insert(MsFlags::MS_RELATIME),
            "norelatime" => flags.remove(MsFlags::MS_RELATIME),
            _ => data.push(opt),
        }
    }
    (data, flags)
}

fn parse_proc_file_systems_file(path: PathBuf) -> Result<Vec<String>> {
    let mut result: Vec<String> = Vec::new();
    let reader = BufReader::new(File::open(path)?);
    for line in reader.lines() {
        let line = line.unwrap_or_default();
        let v: Vec<&str> = line.split_whitespace().collect();
        if v.len() == 2 && v[0] == "nodev" {
            continue;
        }
        let fstype = v[0];
        result.push(fstype.to_string());
    }
    Ok(result)
}

// The Mounter trait is used for dependency injection purposes.
#[cfg_attr(test, mockall::automock)]
trait Mounter {
    fn mount<'a>(
        &'a self,
        source: &'a Path,
        target: &'a Path,
        fstype: Option<&'a str>,
        flags: MsFlags,
        data: Option<&'a str>,
    ) -> Result<()>;
}

// RealMounter is an implementation of the Mounter trait that calls nix::mount::mount for real.
struct RealMounter {}
impl Mounter for RealMounter {
    fn mount(
        &self,
        source: &Path,
        target: &Path,
        fstype: Option<&str>,
        flags: MsFlags,
        data: Option<&str>,
    ) -> Result<()> {
        match nix::mount::mount(Some(source), target, fstype, flags, data) {
            Ok(()) => Ok(()),
            Err(nix::errno::Errno::ENOENT) => {
                if !target.exists() {
                    Err(anyhow!(
                        "No such file or directory: Mount target {:?} doesn't exist",
                        target
                    ))
                } else if !source.exists() {
                    Err(anyhow!(
                        "No such file or directory: Mount source {:?} doesn't exist",
                        source
                    ))
                } else {
                    Err(anyhow!(
                        "No such file or directory: \
                        Unknown reason - both source/target exist"
                    ))
                }
            }
            Err(e) => Err(e.into()),
        }
        .context(format!(
            "mounting {} to {} failed",
            source.display(),
            target.display()
        ))
    }
}

// this is the public facing mount function used by the main and switch_root
pub fn mount(log: Logger, opts: Opts) -> Result<()> {
    let fs_types = parse_proc_file_systems_file(PathBuf::from("/proc/filesystems"))?;
    _mount(log, opts, &fs_types, RealMounter {})
}

fn _mount(log: Logger, opts: Opts, fstypes: &[String], mounter: impl Mounter) -> Result<()> {
    let (data, mut flags) = parse_options(opts.options);
    let source = evaluate_device_spec(&opts.source)?;

    if opts.bind {
        flags.insert(MsFlags::MS_BIND);
        return mounter
            .mount(
                &source,
                opts.target.as_path(),
                None,
                flags,
                Some(data.join(",").as_str()),
            )
            .context("mount failed");
    }

    let fstype = opts.fstype;
    #[cfg(blkid)]
    let fstype = fstype.or_else(|| match blkid::probe_fstype(&source) {
        Ok(fstype) => Some(fstype),
        Err(e) => {
            slog::warn!(
                log,
                "blkid could not determine fstype, trying all available filesystems: {:?}",
                e
            );
            None
        }
    });
    match fstype {
        None => {
            for fstype in fstypes {
                match mounter.mount(
                    &source,
                    opts.target.as_path(),
                    Some(fstype),
                    flags,
                    Some(data.join(",").as_str()),
                ) {
                    Ok(..) => return Ok(()),
                    Err(..) => continue,
                }
            }
            bail!(
                "Filesystem type not provided. I tried many filesystem types I know about with no success: {:?}. Stopping.",
                fstypes,
            );
        }
        Some(fstype) => mounter
            .mount(
                &source,
                opts.target.as_path(),
                Some(&fstype),
                flags,
                Some(data.join(",").as_str()),
            )
            .context("mount failed"),
    }
}

/// Attempt to evaluate a device spec. If blkid is enabled, it will be used,
/// otherwise we have no choice but to assume that the spec is a full device
/// path.
pub fn evaluate_device_spec(spec: &str) -> Result<PathBuf> {
    #[cfg(blkid)]
    return blkid::evaluate_spec(spec)
        .with_context(|| format!("no device matches blkid spec '{}'", spec));

    #[cfg(not(blkid))]
    return Ok(spec.into());
}

#[cfg(test)]
mod tests {
    use super::{parse_options, parse_proc_file_systems_file, MockMounter, Opts, _mount};
    use anyhow::anyhow;
    use anyhow::Result;
    use mockall::predicate::*;
    use nix::mount::MsFlags;
    use std::path::Path;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn parse_options_data_only() {
        assert_eq!(
            parse_options(vec!["subvol=volume".to_string()]),
            (vec!["subvol=volume".to_string()], MsFlags::empty())
        );
    }

    #[test]
    fn parse_options_with_one_flag() {
        assert_eq!(
            parse_options(vec!["subvol=volume".to_string(), "ro".to_string()]),
            (vec!["subvol=volume".to_string()], MsFlags::MS_RDONLY)
        );
    }

    #[test]
    fn parse_options_multiple_flags() {
        assert_eq!(
            parse_options(vec![
                "remount".to_string(),
                "subvol=volume".to_string(),
                "ro".to_string()
            ]),
            (
                vec!["subvol=volume".to_string()],
                MsFlags::MS_RDONLY | MsFlags::MS_REMOUNT
            )
        );
    }

    #[test]
    fn test_read_proc_filesystems() -> Result<()> {
        let tmpdir = TempDir::new()?;
        std::fs::create_dir_all(tmpdir.path().join("proc"))?;
        let fake_proc_filesystems_path = tmpdir.path().join("proc/filesystems");
        std::fs::write(
            fake_proc_filesystems_path.clone(),
            b"nodev   sysfs
nodev   rootfs
nodev   ramfs
nodev   bdev
nodev   proc
nodev   cpuset
nodev   cgroup
nodev   cgroup2
nodev   tmpfs
nodev   devtmpfs
nodev   binfmt_misc
nodev   configfs
nodev   debugfs
nodev   tracefs
nodev   securityfs
nodev   sockfs
nodev   dax
nodev   bpf
nodev   pipefs
nodev   hugetlbfs
nodev   devpts
        ext3
        ext2
        ext4
        vfat
        msdos
nodev   overlay
        xfs
nodev   mqueue
        btrfs
nodev   pstore
nodev   autofs
nodev   efivarfs
        fuseblk
nodev   fuse
nodev   fusectl
nodev   rpc_pipefs\n",
        )?;

        assert_eq!(
            parse_proc_file_systems_file(fake_proc_filesystems_path)?,
            vec![
                "ext3".to_string(),
                "ext2".to_string(),
                "ext4".to_string(),
                "vfat".to_string(),
                "msdos".to_string(),
                "xfs".to_string(),
                "btrfs".to_string(),
                "fuseblk".to_string(),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_mount_cycle_all_fstypes_then_find_btrfs() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let tmpdir = TempDir::new()?;
        std::fs::create_dir_all(tmpdir.path().join("proc"))?;
        let fake_proc_filesystems_path = tmpdir.path().join("proc/filesystems");
        std::fs::write(
            fake_proc_filesystems_path.clone(),
            b"nodev   sysfs
nodev   rootfs
nodev   ramfs
nodev   bdev
nodev   proc
nodev   cpuset
nodev   cgroup
nodev   cgroup2
nodev   tmpfs
nodev   devtmpfs
nodev   binfmt_misc
nodev   configfs
nodev   debugfs
nodev   tracefs
nodev   securityfs
nodev   sockfs
nodev   dax
nodev   bpf
nodev   pipefs
nodev   hugetlbfs
nodev   devpts
        ext3
        ext2
        ext4
        vfat
        msdos
nodev   overlay
        xfs
nodev   mqueue
        btrfs
nodev   pstore
nodev   autofs
nodev   efivarfs
        fuseblk
nodev   fuse
nodev   fusectl
nodev   rpc_pipefs\n",
        )?;

        let opts = Opts {
            bind: false,
            source: "fooSource".to_string(),
            target: PathBuf::from("fooPath"),
            fstype: None,
            options: Vec::new(),
        };
        let fs_types = parse_proc_file_systems_file(fake_proc_filesystems_path)?;
        let mut mock_mounter = MockMounter::new();

        // return success only for btrfs
        mock_mounter
            .expect_mount()
            .withf(
                |
                    source: &Path,
                    target: &Path,
                    fstype: &Option<&str>,
                    flags: &MsFlags,
                    _data: &Option<&str>,
                | {
                    *source == *(Path::new("fooSource"))
                        && target == Path::new("fooPath")
                        && *fstype == Some("btrfs")
                        && *flags == MsFlags::empty()
                },
            )
            .return_once(|_, _, _, _, _| Ok(()));

        // default behavior is to bail for any other filesystem
        mock_mounter
            .expect_mount()
            .returning(|_, _, _, _, _| Err(anyhow!("boom")));

        _mount(log, opts, &fs_types, mock_mounter)?;
        std::fs::remove_dir_all(&tmpdir)?;
        Ok(())
    }
}
