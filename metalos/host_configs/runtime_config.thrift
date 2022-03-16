/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// @oss-disable: include "metalos/host_configs/facebook/proxy/if/ConfigProvider.thrift"

include "metalos/host_configs/package_manifest.thrift"

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs

// Define the acceptable levels of disruptiveness that are allowed during a
// config application. MetalOS can compare this with the set of necessary
// changes in a new Config to determine if the update is safe or not.
//
// Order is extremely important in this enum, and each successive value implies
// that all the previous disruptions are safe.
enum DisruptivenessAllowed {
  // It's safe to switch-root to a new version of the MetalOS rootfs, provided
  // that it does not take down any MetalOS-managed services.
  NoServiceDowntime = 100,
  // It's safe to restart any MetalOS-native service that is switching to a new
  // image/config version.
  ServiceRestart = 200,
  // It's safe to restart the WDS container to a new version/config. This is
  // a separate level because it's more dangerous than restarting MetalOS-native
  // services.
  WdsRestart = 250,
  // It's safe to do anything up to and including rebooting the box. Required
  // for things like kernel version switches.
  FullyDown = 1000,
}

// Describes the complete set of software that should be running on a host, as
// well as any config data that must change during the host's lifecycle.
struct RuntimeConfig {
  // @oss-disable: 1: ConfigProvider.DeploymentRuntimeConfig deployment_specific;
  2: package_manifest.Images images;
  3: string root_pw_hash;
} (rust.exhaustive)

// When trying to apply a runtime config, the packages must already be persisted
// to disk.
safe permanent client exception ImageNotOnDisk {
  1: package_manifest.Image image;
} (rust.exhaustive)

// Options to give to MetalOS when applying a config.
struct ApplyOpts {
  1: DisruptivenessAllowed disruptiveness_allowed;
} (rust.exhaustive)

// Switch into a new Config. All packages must be downloaded ahead-of-time, or
// this request will fail.
struct ApplyConfigRequest {
  1: ApplyOpts opts;
  2: RuntimeConfig config;
} (rust.exhaustive)
