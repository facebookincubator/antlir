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

use anyhow::{bail, Context, Result};
use nix::mount::MsFlags;
use std::fs::File;
use std::io::{BufRead, BufReader};
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Opts {
    pub source: String,
    pub target: PathBuf,
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
        nix::mount::mount(Some(source), target, fstype, flags, data).context("mount failed")
    }
}

// this is the public facing mount function used by the main and switch_root
pub fn mount(opts: Opts) -> Result<()> {
    let fs_types = parse_proc_file_systems_file(PathBuf::from("/proc/filesystems"))?;
    _mount(opts, &fs_types, RealMounter {})
}

fn _mount(opts: Opts, fstypes: &[String], mounter: impl Mounter) -> Result<()> {
    let (data, flags) = parse_options(opts.options);
    let source = source_to_device_path(opts.source)?;

    match opts.fstype {
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
            )
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

/// Parse the source argument into a path where the source device should be
/// mounted. This handles things like LABELs as well as full paths.
pub fn source_to_device_path<S: AsRef<str>>(src: S) -> Result<PathBuf> {
    if let Some(label) = src.as_ref().strip_prefix("LABEL=") {
        // This is a very rudimentary level of support for disk labels.
        // It should probably be using libblkid proper, but this works for the
        // current system setups at least.
        return Ok(format!("/dev/disk/by-label/{}", blkid_encode_string(label)).into());
    }
    // Assume that src is some opaque value that the kernel will know what to do
    // with.
    Ok(src.as_ref().into())
}

// blkid encodes certain tags in a safe encoding that hex-escapes "unsafe"
// characters. This is not a complete implementation and panics when given
// non-ASCII input. See note above: this could be replaced with libblkid
// directly if the need arises.
fn blkid_encode_string<S: AsRef<str>>(s: S) -> String {
    if s.as_ref().chars().any(|ch| !ch.is_ascii()) {
        unimplemented!("this version of blkid_encode_string only supports ASCII");
    }
    let mut encoded = String::with_capacity(s.as_ref().len());
    for ch in s.as_ref().chars() {
        if ch.is_ascii_alphanumeric() {
            encoded.push(ch);
            continue;
        }
        encoded.push_str(format!("\\x{:x}", ch as u8).as_str());
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        blkid_encode_string, parse_options, parse_proc_file_systems_file, source_to_device_path,
        MockMounter, Opts, _mount,
    };
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
    fn blkid_encode_string_samples() {
        assert_eq!("\\x2f", blkid_encode_string("/"));
        assert_eq!("\\x2fboot", blkid_encode_string("/boot"));
        assert_eq!("test\\x20string", blkid_encode_string("test string"));
    }

    #[test]
    fn sources() -> Result<()> {
        assert_eq!(Path::new("/dev/sda"), source_to_device_path("/dev/sda")?);
        assert_eq!(Path::new("sda"), source_to_device_path("sda")?);
        assert_eq!(
            Path::new("/dev/disk/by-label/\\x2f"),
            source_to_device_path("LABEL=/")?
        );
        Ok(())
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

        _mount(opts, &fs_types, mock_mounter)?;
        std::fs::remove_dir_all(&tmpdir)?;
        Ok(())
    }
}
