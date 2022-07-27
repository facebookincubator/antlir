/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// This is the main host configuration description used by MetalOS. Contains all
// the different config pieces for a host, each of which is able to change on
// different lifecycle events.

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs
// @oss-disable: namespace py metalos.host_configs.host
// @oss-disable: namespace go metalos.host_configs.host

include "metalos/host_configs/boot_config.thrift"
include "metalos/host_configs/runtime_config.thrift"
include "metalos/host_configs/provisioning_config.thrift"

// HostConfig is the main entrypoint for a MetalOS host.
struct HostConfig {
  1: provisioning_config.ProvisioningConfig provisioning_config;
  2: boot_config.BootConfig boot_config;
  3: runtime_config.RuntimeConfig runtime_config;
} (rust.exhaustive)
