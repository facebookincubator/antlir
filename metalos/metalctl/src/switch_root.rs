/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{anyhow, Context, Result};
use slog::{debug, o, Logger};
use structopt::StructOpt;
use systemd::{FilePath, Systemd};

use crate::mount::{mount, Opts as MountOpts};

pub const ROOTDISK_DIR: &str = "/rootdisk";

#[derive(StructOpt)]
pub struct Opts {
    snapshot: Option<String>,
}

/// Prior to invoking `systemctl switch-root`, some setup work is required.
/// Mainly, we need to fiddle with mounts so that /sysroot is the rw snapshot for
/// the current bootid. This is necessary so that the newly invoked systemd has
/// the correct root mount point.
pub async fn switch_root(log: Logger, opts: Opts) -> Result<()> {
    let (device, options) = find_rootdisk_device().context("failed to find /rootdisk device")?;
    let mut options: Vec<_> = options.split(',').map(|s| s.to_string()).collect();
    let mut log = log.new(o!());
    if let Some(snapshot) = opts.snapshot {
        log = log.new(o!("snapshot" => snapshot.clone()));
        options = replace_subvol(options, &snapshot)
            .context("failed to replace subvolume in mount options")?;
    }
    std::fs::create_dir("/sysroot").context("failed to mkdir /sysroot")?;
    debug!(
        log,
        "mounting subvolume on {} with options {:?}", device, options
    );
    mount(
        log.clone(),
        MountOpts {
            bind: false,
            source: device.clone(),
            target: "/sysroot".into(),
            fstype: Some("btrfs".into()),
            options: options.clone(),
        },
    )
    .with_context(|| format!("failed to mount '{}' on /sysroot {:?}", device, options))?;

    let sd = Systemd::connect(log.clone()).await?;
    // systemctl daemon-reload is necessary after mounting the
    // to-switch-root-into snapshot at /sysroot, since systemd will
    // automatically reload some unit configuration from /sysroot when running
    // inside the initrd, and this behavior is necessary to pass the correct
    // state of units into the new systemd in the root fs.
    debug!(log, "requesting systemd reload");
    sd.reload()
        .await
        .context("failed to reload systemd units (systemctl daemon-reload)")?;

    debug!(log, "switch-rooting into /sysroot");

    // ask systemd to switch-root to the new root fs
    sd.switch_root(FilePath::new("/sysroot"), FilePath::new(""))
        .await
        .context("failed to trigger switch-root (systemctl switch-root /syroot)")
}

fn replace_subvol<S: AsRef<str>, T: AsRef<str>>(options: Vec<S>, new: T) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for opt in options.into_iter() {
        if opt.as_ref().starts_with("subvolid=") {
            continue;
        }
        let new_op = match opt.as_ref().strip_prefix("subvol=") {
            Some(subvol) => {
                // the subvolume that we are switch-rooting into is guaranteed
                // to be nested under whatever subvolume is already mounted at
                // /rootdisk. So we want to strip off the /rootdisk so that we
                // can get the path relative to the top of the volume
                let new = match new.as_ref().strip_prefix(ROOTDISK_DIR) {
                    Some(subvol) => subvol.trim_start_matches('/'),
                    None => {
                        return Err(anyhow!(
                            "Found subvolume ({}) option but it didn't start with {}",
                            new.as_ref(),
                            ROOTDISK_DIR
                        ));
                    }
                };
                format!("subvol={}/{}", subvol, new)
            }
            None => opt.as_ref().into(),
        };
        out.push(new_op);
    }
    Ok(out)
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
                ROOTDISK_DIR => Some((fields[0].into(), fields[3].into())),
                _ => None,
            }
        })
        .next()
        .ok_or_else(|| anyhow!("{} not in mounts", ROOTDISK_DIR))?;

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
                "/rootdisk/run/boot/0:bootid",
            )
            .expect("Failed to call replace_subvol"),
            vec![
                "rw",
                "relatime",
                "space_cache",
                "subvol=volume/run/boot/0:bootid",
            ],
        )
    }
}
