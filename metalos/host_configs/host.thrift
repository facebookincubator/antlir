/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// This is the main host configuration description used by MetalOS. Eventually a
// core part of this should be marked as the "host identity" and used for
// attestation, but for now this is a mix of what will become the "host
// identity" and some other assorted settings that are needed to setup a
// function system.

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs

// @oss-disable: include "metalos/host_configs/facebook/host.thrift"
// @oss-disable: include "metalos/host_configs/facebook/proxy/if/ConfigProvider.thrift"

include "metalos/host_configs/runtime_config.thrift"

// HostConfig is the main entrypoint for a MetalOS host.
struct HostConfig {
  1: ProvisioningConfig provisioning_config;
  3: runtime_config.RuntimeConfig runtime_config;
} (rust.exhaustive)

// ProvisioningConfig contains immutable host identity information, and anything
// else required for provisioning a box.
struct ProvisioningConfig {
  // @oss-disable: 1: ConfigProvider.DeploymentProvisioningConfig deployment_specific;
  2: HostIdentity identity;
} (rust.exhaustive)

// HostIdentity is the main entrypoint to the Starlark config generator runtime.
// It is the top level struct that should contain all the structured information
// about a host that is necessary for the config generators to materialize
// config files.
struct HostIdentity {
  1: string id;
  2: string hostname;
  4: Network network;
  // @oss-disable: 5: host.HostFacebook facebook;
  // TODO: root_pw_hash should only be set in runtime_config.RuntimeConfig when the
  // expose-to-starlark mechanism is improved to make that easier
  6: string root_pw_hash;
} (
  rust.exhaustive,
  rust.derive = "starlark::values::StarlarkAttrs, metalos_macros::StarlarkInput",
)

// Top-level network settings.
struct Network {
  // TODO: dns should probably just be statically compiled into the image, just
  // as it already is in the initrd
  1: DNS dns;
  2: list<NetworkInterface> interfaces;
} (
  rust.exhaustive,
  rust.derive = "starlark::values::StarlarkAttrs, metalos_macros::StarlarkInput",
)

// Configuration for DNS resolvers.
struct DNS {
  1: list<string> servers;
  2: list<string> search_domains;
} (
  rust.exhaustive,
  rust.derive = "starlark::values::StarlarkAttrs, metalos_macros::StarlarkInput",
)

// Configuration for a single network interface, keyed by MAC Address.
struct NetworkInterface {
  1: string mac;
  2: list<string> addrs;
  3: optional string name;
} (
  rust.exhaustive,
  rust.derive = "starlark::values::StarlarkAttrs, metalos_macros::StarlarkInput",
)
