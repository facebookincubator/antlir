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
    source: PathBuf,
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
    nix::mount::mount(
        Some(&opts.source),
        opts.target.as_path(),
        Some(opts.fstype.as_str()),
        flags,
        Some(data.join(",").as_str()),
    )
    .context("mount failed")
}

#[cfg(test)]
mod tests {
    use super::parse_options;
    use nix::mount::MsFlags;

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
}
