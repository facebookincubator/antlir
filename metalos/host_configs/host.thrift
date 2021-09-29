/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// @oss-disable: include "metalos/host_configs/facebook/host.thrift"

// Host is the main entrypoint to the Starlark config generator runtime. It is
// the top level struct that should contain all the structured information about
// a host that is necessary for the config generators to materialize config
// files. This is designed to (eventually) be serializable by an external
// service and provided directly to a MetalOS host's initrd.
struct Host {
  1: string id;
  2: string hostname;
  3: Network network;
  // @oss-disable: 4: host.HostFacebook facebook;
} (
  rust.derive = "starlark::values::StarlarkAttrs, metalos_derive::StarlarkInput",
)

// Top-level network settings.
struct Network {
  1: DNS dns;
  2: list<NetworkInterface> interfaces;
} (
  rust.derive = "starlark::values::StarlarkAttrs, metalos_derive::StarlarkInput",
)

// Configuration for DNS resolvers.
struct DNS {
  1: list<string> servers;
  2: list<string> search_domains;
} (
  rust.derive = "starlark::values::StarlarkAttrs, metalos_derive::StarlarkInput",
)

// Configuration for a single network interface, keyed by MAC Address.
struct NetworkInterface {
  1: string mac;
  2: list<string> addrs;
  3: optional string name;
} (
  rust.derive = "starlark::values::StarlarkAttrs, metalos_derive::StarlarkInput",
)
