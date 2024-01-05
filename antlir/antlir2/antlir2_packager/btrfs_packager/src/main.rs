/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

extern crate loopdev_erikh as loopdev;

use std::collections::BTreeMap;
use std::fs::create_dir_all;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_btrfs::Subvolume;
use antlir_mount::BoundMounter;
use antlir_mount::Mounter;
use antlir_mount::RealMounter;
use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use bytesize::ByteSize;
use clap::Parser;
use json_arg::JsonFile;
use loopdev::LoopControl;
use loopdev::LoopDevice;
use nix::mount::MsFlags;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use tempfile::TempDir;
use tracing::error;
use tracing::info;
use tracing::warn;

// Otherwise, `mkfs.btrfs` fails with:
//   ERROR: minimum size for each btrfs device is 114294784
pub const MIN_CREATE_BYTES: ByteSize = ByteSize::mib(109);

// Btrfs requires at least this many bytes free in the filesystem
// for metadata
pub const MIN_FREE_BYTES: ByteSize = ByteSize::mib(81);

// Btrfs will not shrink any filesystem below this size
pub const MIN_SHRINK_BYTES: ByteSize = ByteSize::mib(256);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BtrfsSubvol {
    pub sendstream: PathBuf,
    pub layer: PathBuf,
    pub writable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BtrfsSpec {
    pub subvols: BTreeMap<PathBuf, BtrfsSubvol>,
    pub default_subvol: Option<PathBuf>,
    pub compression_level: i32,
    pub label: Option<String>,
    pub free_mb: Option<u64>,
    pub seed_device: bool,
}

pub struct LdHandle {
    device: LoopDevice,
    ld_path: PathBuf,
    closed: bool,
}

impl LdHandle {
    pub fn attach_next_free(target: PathBuf) -> Result<Self> {
        let lc = LoopControl::open().context("Failed to build loop device controller")?;

        // The /dev/loop-control interface being used by this library is
        // supposed to be atomic, but we're clearly getting what looks like race
        // conditions on CI, so just retry a few times and hopefully we'll get a
        // good device before giving up
        retry::retry(
            retry::delay::Fixed::from_millis(100)
                .map(retry::delay::jitter)
                .take(10),
            || {
                let ld = lc
                    .next_free()
                    .context("Failed to find a free loopback device")?;
                Self::attach(ld, target.clone())
            },
        )
        .map_err(|e| match e {
            retry::Error::Operation { error, .. } => error,
            retry::Error::Internal(s) => anyhow::Error::msg(s),
        })
        .context("while trying to attach a loopback device")
    }

    pub fn attach(device: LoopDevice, target: PathBuf) -> Result<Self> {
        let ld_path = device.path().context("loop device had no path")?;
        device
            .attach_file(&target)
            .context(format!("Failed to attach {:?} to {:?}", target, ld_path))?;

        Ok(Self {
            device,
            ld_path,
            closed: false,
        })
    }

    // Give out a bound mounter which can be borrowed at most for 'b (because of self)
    // which will ensure anything that is mounted in this loopback device must be unmounted
    // before the loopback is detatched.
    pub fn mounter<'a, 'b, M>(&'b self, mounter: &'a M) -> BoundMounter<'b, M>
    where
        'a: 'b,
        M: Mounter,
    {
        BoundMounter::new(mounter)
    }

    pub fn detach(mut self) -> Result<()> {
        self.device
            .detach()
            .context("failed to detatch loop device")?;
        self.closed = true;
        Ok(())
    }

    pub fn device(&self) -> &LoopDevice {
        &self.device
    }

    pub fn path(&self) -> &Path {
        &self.ld_path
    }
}

impl Drop for LdHandle {
    fn drop(&mut self) {
        if !self.closed {
            if let Err(e) = self.device.detach() {
                error!("failed to detach loopback {:?} {:#?}", self.ld_path, e);
            }
            self.closed = true;
        }
    }
}

#[derive(Parser, Debug)]
/// Package an image layer into a file
pub(crate) struct PackageArgs {
    #[clap(long)]
    /// Specifications for the packaging
    spec: JsonFile<BtrfsSpec>,
    #[clap(long)]
    /// Path to output the image
    out: PathBuf,
}

pub(crate) fn run_cmd(command: &mut Command) -> Result<std::process::Output> {
    let output = command.output().context("Failed to run command")?;

    match output.status.success() {
        true => Ok(output),
        false => Err(anyhow!("failed to run command {:?}: {:?}", command, output)),
    }
}

pub fn create_empty_file(output: &Path, size: ByteSize) -> Result<()> {
    let file = File::create(output).context("failed to create output file")?;
    file.set_len(size.as_u64())
        .context("while setting file size")?;

    Ok(())
}

/// Loose estimate of the amount of space that these subvols will take - the
/// sendstream size is only correlated to the actual required subvol size in the
/// loopback for a few reasons:
///  * sendstream has different metadata representations
///  * compression levels are almost definitely different
///  * potential deduplication
///
/// Nevertheless, it's a decent input to the estimate of the initial loopback
/// size, since it's likely close to the lower bound of space needed, and
/// overshooting the initial estimate will be rectified by the shrink step.
fn estimate_subvol_size(subvols: &BTreeMap<PathBuf, BtrfsSubvol>) -> Result<ByteSize> {
    let mut total_size = ByteSize::b(0);

    for subvol in subvols.values() {
        let meta = subvol.sendstream.metadata().with_context(|| {
            format!("while checking the size of {}", subvol.sendstream.display())
        })?;
        total_size += ByteSize::b(meta.len());
    }

    Ok(total_size)
}

fn estimate_loopback_device_size(
    subvols: &BTreeMap<PathBuf, BtrfsSubvol>,
    extra_free_space: Option<ByteSize>,
) -> Result<ByteSize> {
    let subvol_size =
        estimate_subvol_size(subvols).context("Failed to calculate size of subvolume layers")?;

    let desired_size = match extra_free_space {
        // If we have been requested to add extra space do so. But we still need the
        // space for the metadata as well
        Some(extra) => subvol_size + MIN_FREE_BYTES + extra,
        // If we don't need extra free space only make room for the metadata
        None => subvol_size + MIN_FREE_BYTES,
    };

    // Since the sendstream sizes are not always a good indicator of how much
    // space it'll take in the loopback, overprovision the loopback a little bit
    // so that we reduce the chance of out-of-space errors - the image will be
    // shrunk as much as possible later on, so this won't really affect the size
    // of the final package.
    // Overprovision our estimate by 20%
    let desired_size = desired_size + (desired_size.as_u64() / 5);

    // btrfs has a minimum size for a FS so we must make sure it's at least that big
    Ok(std::cmp::max(MIN_CREATE_BYTES, desired_size))
}

fn create_btrfs(output_path: &Path, label: Option<String>) -> Result<()> {
    let mut mkfs_cmd = Command::new("mkfs.btrfs");
    mkfs_cmd.arg("--metadata").arg("single");

    if let Some(label) = label {
        mkfs_cmd.arg("--label").arg(label);
    }

    mkfs_cmd.arg(output_path);
    run_cmd(&mut mkfs_cmd).context("failed to mkfs.btrfs")?;
    Ok(())
}

fn discover_subvol_name(temp_dir: &Path) -> Result<PathBuf> {
    let paths: Vec<_> = std::fs::read_dir(temp_dir)
        .context("Failed to list temporary directory")?
        .collect();

    match paths.len() {
        0 => Err(anyhow!("Temporary directory was empty")),
        1 => Ok(paths
            .into_iter()
            .next()
            .expect("Somehow don't have any paths despite checking")
            .context("failed to read only directory in temp dir")?
            .path()),
        n => Err(anyhow!(
            "Too many directories inside temp dir, expected 1 found {}",
            n
        )),
    }
}

fn receive_subvol(
    btrfs_root: &Path,
    subvol_path: &Path,
    subvol: &BtrfsSubvol,
) -> Result<Subvolume> {
    // We need to receive the subvolume into a temp directory because btrfs doesn't let us
    // name the destination directory anything other than the subvolume name.
    let recv_target = TempDir::new_in(btrfs_root)
        .context("failed to create temporary directory for btrfs-receive")?;
    let subvol_parent_path_relative = subvol_path
        .parent()
        .context("Expected subvolume path to have a parent directory")?
        .strip_prefix("/")
        .context("Subvols must start with /")?;

    let subvol_parent_path = btrfs_root.join(subvol_parent_path_relative);
    let subvol_final_path = btrfs_root.join(
        subvol_path
            .strip_prefix("/")
            .context("Subvols must start with /")?,
    );

    let sendstream = File::open(&subvol.sendstream)
        .with_context(|| format!("while opening sendstream {}", subvol.sendstream.display()))?;

    let mut btrfs_recv = Command::new("btrfs")
        .arg("receive")
        .arg(recv_target.path())
        .stdin(sendstream)
        .spawn()
        .context("Failed to spawn btrfs receive")?;

    ensure!(btrfs_recv.wait()?.success(), "btrfs-recv failed");

    create_dir_all(&subvol_parent_path).context(format!(
        "Failed to create destination directory {:?}",
        subvol_parent_path
    ))?;

    // Btrfs receive foo will make foo/<volume name> so we receive into a temporary directory and
    // then move the final volume to it's correct place (final_target) after we receive.
    // We don't know the volume name here so we go find it in our tmp directory
    let new_subvol_path =
        discover_subvol_name(recv_target.path()).context("Failed to find received subvol path")?;

    let mut new_subvol =
        Subvolume::open(&new_subvol_path).context("failed to create subvol from new directory")?;
    new_subvol
        .set_readonly(false)
        .context("failed to mark new subvol as RW")?;

    std::fs::rename(&new_subvol_path, &subvol_final_path).context(format!(
        "Failed to rename tmp dir {:?} to {:?}",
        new_subvol_path, subvol_final_path
    ))?;

    let renamed_subvol =
        Subvolume::open(&subvol_final_path).context("failed to recreate subvol after rename")?;

    drop(recv_target);
    Ok(renamed_subvol)
}

fn receive_subvols(
    btrfs_root: &Path,
    subvols: BTreeMap<PathBuf, BtrfsSubvol>,
) -> Result<BTreeMap<PathBuf, (Subvolume, BtrfsSubvol)>> {
    let mut subvols = Vec::from_iter(subvols);
    subvols.sort_by(|(a_path, _), (b_path, _)| a_path.cmp(b_path));

    let mut built_subvols = BTreeMap::new();
    for (subvol_path, subvol) in subvols {
        let built_subvol = receive_subvol(btrfs_root, &subvol_path, &subvol)
            .context(format!("Failed to setup subvolume: {:?}", subvol_path))?;
        built_subvols.insert(subvol_path, (built_subvol, subvol));
    }

    for (subvol_path, (built_subvol, subvol)) in built_subvols.iter_mut() {
        built_subvol
            .set_readonly(!subvol.writable.unwrap_or(false))
            .context(format!("failed to set readability for {:?}", subvol_path))?;
    }

    Ok(built_subvols)
}

static MIN_DEV_SIZE_RE: Lazy<Regex> = Lazy::new(|| {
    // reports something like
    // 146145280 bytes (139.38MiB)
    Regex::new(r#"^(?P<min_bytes>\d+)\s+bytes\s+"#).expect("regex failed to compile")
});

enum ShrinkResult {
    ShrunkTo(ByteSize),
    TooSmall,
}

fn shrink_fs_once(mountpoint: &Path, free_space: Option<ByteSize>) -> Result<ShrinkResult> {
    let out = Command::new("btrfs")
        .arg("inspect-internal")
        .arg("min-dev-size")
        .arg(mountpoint)
        .output()
        .context("failed to check size")?;
    ensure!(
        out.status.success(),
        "btrfs inspect-internal min-dev-size failed"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let cap = MIN_DEV_SIZE_RE
        .captures(&stdout)
        .context("while parsing btrfs output")?;
    let min_dev_size = ByteSize::b(
        cap.name("min_bytes")
            .expect("min_bytes match must exist")
            .as_str()
            .parse::<u64>()
            .expect("min_bytes must be integer"),
    );

    info!("btrfs reports minimum device size {min_dev_size}");

    let new_size = match free_space {
        Some(free_space) => {
            info!("adding {free_space} as packager requested extra space");
            min_dev_size + free_space
        }
        None => min_dev_size,
    };

    if new_size < MIN_SHRINK_BYTES {
        warn!("fs is smaller than minimum {MIN_SHRINK_BYTES}, cannot shrink any further");
        return Ok(ShrinkResult::TooSmall);
    }

    let new_size = std::cmp::max(MIN_SHRINK_BYTES, new_size);
    info!("shrinking fs to {new_size}");

    let out = Command::new("btrfs")
        .arg("filesystem")
        .arg("resize")
        .arg(new_size.as_u64().to_string())
        .arg(mountpoint)
        .output()
        .context("failed to run btrfs filesystem resize")?;
    ensure!(
        out.status.success(),
        "btrfs filesystem resize failed\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    Ok(ShrinkResult::ShrunkTo(new_size))
}

fn shrink_fs(mountpoint: &Path, free_space: Option<ByteSize>) -> Result<ShrinkResult> {
    let mut iteration = 0;
    let mut prev_size = None;
    // 'btrfs filesystem resize' might end up being able to get us a smaller
    // total size if we do it a few times, so we keep calling shrink_fs_once
    // until the size stops changing
    loop {
        let span = tracing::trace_span!("shrink_fs", iteration = iteration);
        let _enter = span.enter();
        match shrink_fs_once(mountpoint, free_space)? {
            ShrinkResult::ShrunkTo(new_new_size) => {
                // as soon as we stop being able to shrink the fs further, we can say
                // that this is the true minimum
                if Some(new_new_size) == prev_size {
                    info!("last iteration did not shrink smaller than {new_new_size}, stopping...");
                    return Ok(ShrinkResult::ShrunkTo(new_new_size));
                } else {
                    info!("shrank to {new_new_size}, but fs might still be shrinking");
                    prev_size = Some(new_new_size);
                }
            }
            ShrinkResult::TooSmall => {
                return Ok(match prev_size {
                    Some(prev_size) => ShrinkResult::ShrunkTo(prev_size),
                    None => ShrinkResult::TooSmall,
                });
            }
        }
        iteration += 1;
    }
}

fn make_btrfs_package<M>(
    mounter: M,
    output_path: &Path,
    subvols: BTreeMap<PathBuf, BtrfsSubvol>,
    default_subvol: Option<PathBuf>,
    label: Option<String>,
    compression_level: i32,
    extra_free_space: Option<ByteSize>,
    seed_device: bool,
) -> Result<()>
where
    M: Mounter,
{
    let size = estimate_loopback_device_size(&subvols, extra_free_space)
        .context("Failed to calculate minimum size for loopback file")?;

    create_empty_file(output_path, size).context("Failed to create empty output file")?;

    create_btrfs(output_path, label).context("Failed to create btrfs filesystem")?;

    let ld = LdHandle::attach_next_free(output_path.to_path_buf())
        .context("failed to create loopback device")?;
    let ld_mounter = ld.mounter(&mounter);

    let tmp_mountpoint = TempDir::new().context("failed to create mountpoint directory")?;

    let mount_handle = ld_mounter
        .mount(
            ld.path(),
            tmp_mountpoint.path(),
            Some("btrfs"),
            MsFlags::empty(),
            Some(
                format!(
                    "compress-force=zstd:{},discard,nobarrier",
                    compression_level
                )
                .as_str(),
            ),
        )
        .context("Failed to mount output btrfs")?;

    let subvols =
        receive_subvols(mount_handle.mountpoint(), subvols).context("failed to recv subvols")?;

    if let Some(default_subvol) = default_subvol {
        let default_subvol_id = if default_subvol == Path::new("/") {
            5
        } else {
            match subvols.get(&default_subvol) {
                Some((built_default_subvol, _)) => built_default_subvol.id(),
                None => {
                    return Err(anyhow!(
                        "Default subvolume {:?} not found in subvolumes",
                        default_subvol
                    ));
                }
            }
        };

        run_cmd(
            Command::new("btrfs")
                .arg("subvolume")
                .arg("set-default")
                .arg(default_subvol_id.to_string())
                .arg(mount_handle.mountpoint()),
        )
        .context("setting default subvolume")?;
    }

    // make sure no file descriptors to these subvolumes are open before
    // unmounting the package
    drop(subvols);

    // The loopback image is almost definitely far larger than it needs to be,
    // so let's try to shrink it as much as possible.
    let dev_size = shrink_fs(mount_handle.mountpoint(), extra_free_space)
        .context("while shrinking loopback")?;

    mount_handle.umount(true).context("failed to umount")?;

    if seed_device {
        run_cmd(Command::new("btrfstune").arg("-S").arg("1").arg(ld.path()))
            .context("while setting seed device")?;
    }

    ld.detach().context("failed to detatch loopback device")?;

    match dev_size {
        ShrinkResult::ShrunkTo(dev_size) => {
            info!("shrinking image file to {dev_size}");
            let image = std::fs::OpenOptions::new()
                .write(true)
                .open(output_path)
                .with_context(|| format!("while opening {}", output_path.display()))?;
            image
                .set_len(dev_size.as_u64())
                .context("while truncating image file")?;
        }
        ShrinkResult::TooSmall => {
            warn!(
                "image file is possibly bigger than it needs to be, but btrfs won't let us shrink it below {MIN_SHRINK_BYTES}"
            );
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let args = PackageArgs::parse();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let spec = args.spec.into_inner();

    make_btrfs_package(
        RealMounter {},
        &args.out,
        spec.subvols,
        spec.default_subvol,
        spec.label,
        spec.compression_level,
        spec.free_mb.map(ByteSize::mib),
        spec.seed_device,
    )
}
