/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use nix::mount::mount;
use nix::mount::MsFlags;
use nix::sched::unshare;
use nix::sched::CloneFlags;

/// Unshare into a new mount namespace and make it private so that any new
/// mounts can't escape back ot the parent mount namespace.
pub fn unshare_and_privatize_mount_ns() -> std::io::Result<()> {
    // Be careful to isolate this process from the host mount namespace in
    // case anything weird is going on
    unshare(CloneFlags::CLONE_NEWNS)?;

    // Remount / as private so that we don't let any changes escape back
    // to the parent mount namespace (basically equivalent to `mount
    // --make-rprivate /`)
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    )?;
    Ok(())
}
