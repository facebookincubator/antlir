/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Describe all the different kinds of software packages in MetalOS. Every
// package contains at least one PackageId that informs MetalOS how to download
// the image.
// Package structs may be composed of multiple underlying images, and may come
// with more structured data to augment the images. For example, a Kernel
// package includes both a package id and a cmdline to use.

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs
// @oss-disable: namespace go metalos.host_configs.packages

union PackageId {
  // Fully resolved UUID that always points to the same exact package version
  1: string uuid;
  // Arbitrary string that points to a specific package version at a given point
  // in time. It may change to point to a different package version at any time,
  // so it must be resolved before being saved / provided to a MetalOS host.
  // This will fail to deserialize using the safe Rust wrappers, unless
  // explicitly allowed by the calling code.
  2: string tag;
}

// How to download an individual package. Each package is uniquely identified by
// a (name, id) pair, and can optionally be redirected to a specific URI for
// development purposes.
struct Package {
  1: string name;
  2: PackageId id;
  3: optional string override_uri;
  4: Kind kind;
  5: Format format;
} (rust.exhaustive)

enum Kind {
  ROOTFS = 1,
  KERNEL = 2,
  INITRD = 3,
  IMAGING_INITRD = 4,
  SERVICE = 5,
  SERVICE_CONFIG_GENERATOR = 6,
  GPT_ROOT_DISK = 7,
  BOOTLOADER = 8,
}

enum Format {
  SENDSTREAM = 1,
  FILE = 2,
}

// TODO(T121059111) make this a union
struct PackageStatus {
  1: Package pkg;
  2: InstallationStatus installation_status;
  3: optional string error;
} (rust.exhaustive)

enum InstallationStatus {
  SUCCESS = 1,
  FAILED_TO_DOWNLOAD = 2,
  FAILED_TO_INSTALL = 3,
  PACKAGE_NOT_FOUND = 4,
  UNKNOWN = 5,
}
