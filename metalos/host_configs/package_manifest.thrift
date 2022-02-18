/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs

// This informs MetalOS where an image should be persisted to on disk, as well
// as giving MetalOS the ability to implement per-kind quotas etc.
enum Kind {
  ROOTFS = 1,
  CONFIG = 2,
  KERNEL = 3,
  SERVICE = 4,
  WDS = 5,
  GPT_ROOTDISK = 6,
}

// An image package that should be materialized in a btrfs subvolume on disk.
// Currently these are always zstd-compressed btrfs sendstreams.
struct Image {
  // The (name, id) pair must be able to uniquely (globally) identify this
  // image, and tell metalos how to download it.
  1: string name;
  2: string id;
  3: Kind kind;
  // Explicitly point this image at an alternate location (eg a devserver
  // instead of prod fbpkg)
  4: optional string override_uri;
} (rust.exhaustive)

// Returned when an image could not be downloaded.
safe transient server exception DownloadError {
  1: Image image;
  2: string message;
} (rust.exhaustive)

// Complete Manifest of software to be downloaded.
struct Manifest {
  1: list<Image> images;
} (rust.exhaustive)
