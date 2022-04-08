/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs

include "metalos/host_configs/packages.thrift"
// @oss-disable: include "metalos/host_configs/facebook/host.thrift"
// @oss-disable: include "metalos/host_configs/facebook/proxy/if/deployment_specific.thrift"

// ProvisioningConfig contains immutable host identity information, and anything
// else required for provisioning a box. This is only allowed to change on
// reprovisions, not during a host's normal lifecycle.
struct ProvisioningConfig {
  // @oss-disable: 1: deployment_specific.DeploymentProvisioningConfig deployment_specific;
  2: HostIdentity identity;
  // initial root pw hash, will be rotated at runtime outside of the typical
  // MetalOS update lifecycles
  3: string root_pw_hash;
  4: packages.GptRootdisk gpt_root_disk;
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
} (rust.exhaustive)

// Top-level network settings.
struct Network {
  // TODO: dns should probably just be statically compiled into the image, just
  // as it already is in the initrd
  1: DNS dns;
  2: list<NetworkInterface> interfaces;
} (rust.exhaustive)

// Configuration for DNS resolvers.
struct DNS {
  1: list<string> servers;
  2: list<string> search_domains;
} (rust.exhaustive)

// Configuration for a single network interface, keyed by MAC Address.
struct NetworkInterface {
  1: string mac;
  2: list<string> addrs;
  3: optional string name;
} (rust.exhaustive)
