/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs
// @oss-disable: namespace go metalos.host_configs.boot_config

include "metalos/host_configs/packages.thrift"
// @oss-disable: include "metalos/host_configs/facebook/proxy/if/deployment_specific.thrift"

// Complete boot-time config for a MetalOS host. Requires a reboot to update.
struct BootConfig {
  // @oss-disable: 1: deployment_specific.DeploymentBootConfig deployment_specific;
  2: packages.Package rootfs;
  3: Kernel kernel;
  4: packages.Package initrd;
  5: optional Bootloader bootloader;
} (rust.exhaustive)

struct Kernel {
  1: packages.Package pkg;
  2: string cmdline;
} (rust.exhaustive)

struct Bootloader {
  1: packages.Package pkg;
  2: string cmdline;
} (rust.exhaustive)
