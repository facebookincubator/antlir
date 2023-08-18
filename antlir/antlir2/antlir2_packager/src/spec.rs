/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec {
    Btrfs(crate::btrfs::Btrfs),
    Cpio(crate::cpio::Cpio),
    Rpm(crate::rpm::Rpm),
    Sendstream(crate::sendstream::Sendstream),
    Squashfs(crate::squashfs::Squashfs),
    Tar(crate::tar::Tar),
    Vfat(crate::vfat::Vfat),
}
