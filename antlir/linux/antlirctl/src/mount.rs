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
use std::path::PathBuf;

use anyhow::{Context, Result};
use nix::mount::MsFlags;
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Opts {
    source: String,
    target: PathBuf,
    #[structopt(short = "t")]
    fstype: String,
    #[structopt(short, require_delimiter(true))]
    options: Vec<String>,
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
            _ => data.push(opt),
        }
    }
    (data, flags)
}

pub fn mount(opts: Opts) -> Result<()> {
    let (data, flags) = parse_options(opts.options);
    let source = source_to_device_path(opts.source)?;
    nix::mount::mount(
        Some(&source),
        opts.target.as_path(),
        Some(opts.fstype.as_str()),
        flags,
        Some(data.join(",").as_str()),
    )
    .context("mount failed")
}

/// Parse the source argument into a path where the source device should be
/// mounted. This handles things like LABELs as well as full paths.
pub fn source_to_device_path<S: AsRef<str>>(src: S) -> Result<PathBuf> {
    if let Some(label) = src.as_ref().strip_prefix("LABEL=") {
        // This is a very rudimentary level of support for disk labels.
        // It should probably be using libblkid proper, but this works for the
        // current system setups at least.
        eprintln!("label = {:?}", label);
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
    use super::{blkid_encode_string, parse_options, source_to_device_path};
    use anyhow::Result;
    use nix::mount::MsFlags;
    use std::path::Path;

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
}
