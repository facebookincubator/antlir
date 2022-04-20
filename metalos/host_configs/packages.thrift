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

// How to download an individual package. Each package is uniquely identified by
// a (name, uuid) pair, and can optionally be redirected to a specific URI for
// development purposes.
struct PackageId {
  1: string name;
  2: string uuid;
  3: optional string override_uri;
} (rust.exhaustive)

struct Rootfs {
  1: PackageId id;
} (rust.exhaustive)

struct Initrd {
  1: PackageId id;
} (rust.exhaustive)

struct Kernel {
  1: PackageId kernel;
  2: string cmdline;
} (rust.exhaustive)

struct Service {
  1: PackageId service_id;
  2: optional PackageId generator_id;
} (rust.exhaustive, rust.ord)

struct GptRootdisk {
  1: PackageId id;
} (rust.exhaustive)

struct Status {
  1: InstallationStatus installation_status;
  2: optional string error;
} (rust.exhaustive)

enum InstallationStatus {
  SUCCESS = 1,
  FAILED_TO_DOWNLOAD = 2,
  FAILED_TO_INSTALL = 3,
  PACKAGE_NOT_FOUND = 4,
  UNKNOWN = 5,
}
