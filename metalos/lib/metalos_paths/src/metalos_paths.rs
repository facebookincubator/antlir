/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Various MetalOS on-disk paths in one place to prevent copy-pasta from
//! proliferating across many different MetalOS libraries.

use std::path::Path;

/// Control subvolume. This is treated as the root subvolume of the disk (but is
/// possibly not actually subvolid=5) and is used to manage images and runtime
/// subvolumes on a running system, while a per-boot snapshot is mounted on `/`
pub fn control() -> &'static Path {
    Path::new("/run/fs/control")
}

/// Root directory for image storage. Images are stored hierarchically in here
/// based on their type, but should all be rooted under this directory.
pub fn images() -> &'static Path {
    Path::new("/run/fs/control/image")
}

/// Root directory for runtime storage. This contains all per-host runtime
/// storage data, including all the native service volumes and any other state
/// that MetalOS keeps track of internally.
pub fn runtime() -> &'static Path {
    Path::new("/run/fs/control/run")
}

/// Root directory for MetalOS-internal persistent state.
pub fn metalos_state() -> &'static Path {
    Path::new("/run/fs/control/run/state/metalos")
}
