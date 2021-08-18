/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::{Context, Error, Result};
use slog::{debug, o, Logger};
use structopt::StructOpt;

use crate::mount::{mount, Opts as MountOpts};

#[derive(StructOpt)]
pub struct Opts {
    snapshot: String,
}

/// Prior to invoking `systemctl switch-root`, some setup work is required.
/// Mainly, we need to fiddle with mounts so that /sysroot is the rw snapshot for
/// the current bootid. This is necessary so that the newly invoked systemd has
/// the correct root mount point.
pub fn switch_root(log: Logger, opts: Opts) -> Result<()> {
    let log = log.new(o!("snapshot" => opts.snapshot.clone()));
    let (device, options) = find_rootdisk_device().context("failed to find /rootdisk device")?;
    let options: Vec<_> = options.split(',').map(|s| s.to_string()).collect();
    let options = replace_subvol(options, &opts.snapshot);
    // TODO: for vmtest, no matter how we mount the rootfs, /proc/mounts will
    // always report that the device is /dev/vda. There are ways to work around
    // this to generically find the writable device (/dev/vdb) if this hack
    // stops working at some point, but for now this is way easier. For example:
    // seedroot.service can mount /dev/vdb instead of just remounting /dev/vda,
    // and then /sys/fs/btrfs/$uuid/devices will show the correct writable
    // device
    let device = match device.as_str() {
        "/dev/vda" => "/dev/vdb".into(),
        _ => device,
    };
    std::fs::create_dir("/sysroot").context("failed to mkdir /sysroot")?;
    debug!(log, "mounting subvolume on {}", device);
    mount(MountOpts {
        source: device.clone(),
        target: "/sysroot".into(),
        fstype: "btrfs".into(),
        options: options.clone(),
    })
    .with_context(|| {
        format!(
            "failed to mount subvol '{}' on '{}' at /sysroot {:?}",
            opts.snapshot, device, options,
        )
    })?;
    // systemctl daemon-reload is necessary after mounting the
    // to-switch-root-into snapshot at /sysroot, since systemd will
    // automatically reload some unit configuration from /sysroot when running
    // inside the initrd, and this behavior is necessary to pass the correct
    // state of units into the new systemd in the root fs.
    // TODO: it would be nice to handle communication with systemd
    // post-generator with the dbus api
    Command::new("systemctl")
        .arg("daemon-reload")
        .spawn()
        .context("failed to spawn 'systemctl daemon-reload'")?
        .wait()
        .context("'systemctl daemon-reload' failed")?;

    debug!(log, "switch-rooting into /sysroot");
    let error = Command::new("systemctl")
        .args(&["--no-block", "switch-root", "/sysroot"])
        .exec();
    // We'll attempt to return an nice Err result, but the process may be in a
    // corrupt state and if the main binary attempts any cleanup it may fail
    // even more (but we've already lost, so might as well try).
    // https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#notes
    Err(Error::new(error))
}

fn replace_subvol<S: AsRef<str>, T: AsRef<str>>(options: Vec<S>, new: T) -> Vec<String> {
    options
        .into_iter()
        .filter_map(|opt| {
            if opt.as_ref().starts_with("subvolid=") {
                return None;
            }
            match opt.as_ref().strip_prefix("subvol=") {
                Some(subvol) => {
                    // the subvolume that we are switch-rooting into is guaranteed
                    // to be nested under whatever subvolume is already mounted at
                    // /sysroot
                    let new = new.as_ref().trim_start_matches('/');
                    Some(format!("subvol={}/{}", subvol, new))
                }
                None => Some(opt.as_ref().into()),
            }
        })
        .collect()
}

fn find_rootdisk_device() -> Result<(String, String)> {
    let mounts = std::fs::read_to_string("/proc/mounts").context("failed to read /proc/mounts")?;
    let (dev, opts) = parse_rootdisk_device(mounts)?;
    // attempt to resolve any symlinks or otherwise non-canonical paths
    let dev = std::fs::canonicalize(&dev)
        .map(|path| path.to_string_lossy().into())
        .unwrap_or(dev);
    Ok((dev, opts))
}

/// Parse /proc/mounts output to find the device which is mounted at /rootdisk
fn parse_rootdisk_device(mounts: String) -> Result<(String, String)> {
    let (mut dev, opts): (String, String) = mounts
        .lines()
        .filter_map(|l| {
            let fields: Vec<_> = l.split_whitespace().collect();
            match fields[1] {
                "/rootdisk" => Some((fields[0].into(), fields[3].into())),
                _ => None,
            }
        })
        .next()
        .ok_or(Error::msg("/rootdisk not in mounts"))?;

    // /proc/mounts escapes characters with octal
    if dev.contains('\\') {
        let mut octal_chars: Option<String> = None;
        dev = dev.chars().fold("".to_string(), |mut s, ch| {
            if let Some(ref mut oc) = octal_chars {
                oc.push(ch);
                if oc.len() == 3 {
                    let escaped = u32::from_str_radix(&oc, 8)
                        .with_context(|| format!("'{}' is not a valid octal number", &oc))
                        .unwrap();
                    let escaped = char::from_u32(escaped)
                        .with_context(|| format!("0o{} is not a valid character", escaped))
                        .unwrap();
                    s.push(escaped);

                    octal_chars = None;
                }
            } else if ch == '\\' {
                octal_chars = Some(String::new());
            } else {
                s.push(ch);
            }
            s
        });
    }
    Ok((dev, opts))
}

#[cfg(test)]
mod tests {
    use super::{parse_rootdisk_device, replace_subvol};
    use anyhow::Result;

    #[test]
    fn rootdisk_device() -> Result<()> {
        let input = r#"rootfs / rootfs rw 0 0
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
devtmpfs /dev devtmpfs rw,nosuid,size=4096k,nr_inodes=65536,mode=755 0 0
securityfs /sys/kernel/security securityfs rw,nosuid,nodev,noexec,relatime 0 0
tmpfs /dev/shm tmpfs rw,nosuid,nodev 0 0
devpts /dev/pts devpts rw,nosuid,noexec,relatime,gid=5,mode=620,ptmxmode=000 0 0
tmpfs /run tmpfs rw,nosuid,nodev,size=806188k,nr_inodes=819200,mode=755 0 0
cgroup2 /sys/fs/cgroup cgroup2 rw,nosuid,nodev,noexec,relatime,nsdelegate,memory_recursiveprot 0 0
pstore /sys/fs/pstore pstore rw,nosuid,nodev,noexec,relatime 0 0
bpf /sys/fs/bpf bpf rw,nosuid,nodev,noexec,relatime,mode=700 0 0
fs0 /data/users/vmagro/fbsource 9p ro,dirsync,relatime,loose,access=client,trans=virtio 0 0
fs2 /data/users/vmagro/scratch/dataZusersZvmagroZfbsource/buck-image-out 9p ro,dirsync,relatime,loose,access=client,trans=virtio 0 0
fs1 /mnt/gvfs 9p ro,dirsync,relatime,loose,access=client,trans=virtio 0 0
usr-local-fbcode /usr/local/fbcode 9p ro,dirsync,relatime,loose,access=client,trans=virtio 0 0
/dev/vdc /vmtest btrfs ro,relatime,space_cache,subvolid=256,subvol=/volume 0 0
/dev/vda /rootdisk btrfs rw,relatime,space_cache,subvolid=256,subvol=/volume 0 0
kernel-modules /rootdisk/usr/lib/modules/5.2.9-229_fbk15_hardened_4185_g357f49b36602 9p ro,dirsync,relatime,loose,access=client,trans=virtio 0 0"#.to_string();
        assert_eq!(
            parse_rootdisk_device(input)?,
            (
                "/dev/vda".into(),
                "rw,relatime,space_cache,subvolid=256,subvol=/volume".into()
            )
        );
        let input = r#"rootfs / rootfs rw 0 0
/dev/vdc /vmtest btrfs ro,relatime,space_cache,subvolid=256,subvol=/volume 0 0
/dev/disk/by-label/\134x2f /rootdisk btrfs rw,relatime,space_cache,subvolid=256,subvol=/volume 0 0
kernel-modules /rootdisk/usr/lib/modules/5.2.9-229_fbk15_hardened_4185_g357f49b36602 9p ro,dirsync,relatime,loose,access=client,trans=virtio 0 0"#.to_string();
        assert_eq!(
            parse_rootdisk_device(input)?,
            (
                r"/dev/disk/by-label/\x2f".into(),
                "rw,relatime,space_cache,subvolid=256,subvol=/volume".into()
            )
        );
        Ok(())
    }

    #[test]
    fn subvol_replacements() {
        assert_eq!(
            replace_subvol(
                vec![
                    "rw",
                    "relatime",
                    "space_cache",
                    "subvolid=256",
                    "subvol=volume"
                ],
                "/var/lib/antlir/boot/per-boot-subvol"
            ),
            vec![
                "rw",
                "relatime",
                "space_cache",
                "subvol=volume/var/lib/antlir/boot/per-boot-subvol"
            ],
        )
    }
}
