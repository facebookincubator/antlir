/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::fs::create_dir_all;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_package_lib::create_empty_file;
use antlir2_package_lib::run_cmd;
use antlir2_package_lib::BtrfsSpec;
use antlir2_package_lib::BtrfsSubvol;
use antlir_mount::BoundMounter;
use antlir_mount::Mounter;
use antlir_mount::RealMounter;
use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use btrfs::Subvolume;
use bytesize::ByteSize;
use clap::Parser;
use json_arg::JsonFile;
use loopdev::LoopControl;
use loopdev::LoopDevice;
use nix::mount::MsFlags;
use tempfile::TempDir;
use tracing_subscriber::prelude::*;

// Otherwise, `mkfs.btrfs` fails with:
//   ERROR: minimum size for each btrfs device is 114294784
pub const MIN_CREATE_BYTES: ByteSize = ByteSize::mib(109);

// Btrfs requires at least this many bytes free in the filesystem
// for metadata
pub const MIN_FREE_BYTES: ByteSize = ByteSize::mib(81);

pub struct LdHandle {
    device: LoopDevice,
    ld_path: PathBuf,
    closed: bool,
}

impl LdHandle {
    pub fn attach_next_free(target: PathBuf) -> Result<Self> {
        let lc = LoopControl::open().context("Failed to build loop device controller")?;
        let ld = lc
            .next_free()
            .context("Failed to find a free loopback device")?;

        Self::attach(ld, target)
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
        self.closed = true;
        self.device
            .detach()
            .context("failed to detatch loop device")
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
                eprintln!("failed to detach loopback {:?} {:#?}", self.ld_path, e);
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

fn calculate_subvol_sizes(subvols: &BTreeMap<PathBuf, BtrfsSubvol>) -> Result<ByteSize> {
    let mut total_size = ByteSize::b(0);

    // TODO(vmagro): AFAICT, this size calculation assumes that the compression
    // level of the build subvolume is on par with the compression level of the
    // package. While this is largely true today, it does not seem that
    // reliable. Probably better would be to compress a sendstream at the same
    // level and using the size of that.
    for (subvol_path, subvol) in subvols.iter() {
        let du_out = run_cmd(
            Command::new("du")
                .arg("--block-size=1")
                .arg("--summarize")
                // Hack alert: `--one-file-system` works around the fact that we
                // may have host mounts inside the image, which we shouldn't count.
                .arg("--one-file-system")
                .arg(&subvol.layer),
        )
        .context(format!("Failed to get size of subvol {:?}", subvol_path))?;

        let du_str =
            std::str::from_utf8(&du_out.stdout).context("Failed to parse du output as utf-8")?;
        let size = ByteSize::b(match du_str.split_once('\t') {
            Some((left, _)) => left
                .parse()
                .context(format!("Failed to parse du output to int: {}", left))?,
            None => {
                return Err(anyhow!("Unable to find tab in du output: {}", du_str));
            }
        });
        total_size += size;
    }

    Ok(total_size)
}

fn calculate_new_fs_size(
    subvols: &BTreeMap<PathBuf, BtrfsSubvol>,
    extra_free_space: Option<ByteSize>,
) -> Result<ByteSize> {
    let subvol_size =
        calculate_subvol_sizes(subvols).context("Failed to calculate size of subvolume layers")?;

    let desired_size = match extra_free_space {
        // If we have been requested to add extra space do so. But we still need the
        // space for the metadata as well
        Some(extra) => subvol_size + MIN_FREE_BYTES + extra,
        // If we don't need extra free space only make room for the metadata
        None => subvol_size + MIN_FREE_BYTES,
    };

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
        Subvolume::get(&new_subvol_path).context("failed to create subvol from new directory")?;
    new_subvol
        .set_readonly(false)
        .context("failed to mark new subvol as RW")?;

    std::fs::rename(&new_subvol_path, &subvol_final_path).context(format!(
        "Failed to rename tmp dir {:?} to {:?}",
        new_subvol_path, subvol_final_path
    ))?;

    let renamed_subvol =
        Subvolume::get(&subvol_final_path).context("failed to recreate subvol after rename")?;

    drop(recv_target);
    Ok(renamed_subvol)
}

fn send_and_receive_subvols(
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

fn make_btrfs_package<M>(
    mounter: M,
    output_path: &Path,
    subvols: BTreeMap<PathBuf, BtrfsSubvol>,
    default_subvol: PathBuf,
    label: Option<String>,
    compression_level: i32,
    extra_free_space: Option<ByteSize>,
) -> Result<()>
where
    M: Mounter,
{
    let size = calculate_new_fs_size(&subvols, extra_free_space)
        .context("Failed to calculate minimum size for new btrfs fs")?;

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

    let subvols = send_and_receive_subvols(mount_handle.mountpoint(), subvols)
        .context("failed to send/recv subvols")?;

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

    mount_handle.umount(true).context("failed to umount")?;
    ld.detach().context("failed to detatch loopback device")?;

    Ok(())
}

fn main() -> Result<()> {
    let args = PackageArgs::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::Layer::default()
                .event_format(
                    tracing_glog::Glog::default()
                        .with_span_context(true)
                        .with_timer(tracing_glog::LocalTime::default()),
                )
                .fmt_fields(tracing_glog::GlogFields::default()),
        )
        .with(tracing_subscriber::EnvFilter::from_default_env())
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
    )
}
