/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs
// @oss-disable: namespace py metalos.host_configs.runtime_config
// @oss-disable: namespace go metalos.host_configs.runtime_config

// @oss-disable: include "metalos/host_configs/facebook/proxy/if/deployment_specific.thrift"

include "metalos/host_configs/packages.thrift"

// Describes the complete set of software that should be running on a host, as
// well as any config data that must change during a single boot cycle (not
// requiring a reboot).
struct RuntimeConfig {
  // @oss-disable: 1: deployment_specific.DeploymentRuntimeConfig deployment_specific;
  // The complete set of services that should be running on a host after this
  // runtime config is committed.
  2: list<Service> services;
} (rust.exhaustive)

enum ServiceType {
  // NATIVE services execute inside a heavily read-only sandboxed environment with
  // specific writable paths for logs, state, etc.
  NATIVE = 0,
  // OS services execute inside the main OS layer and is meant to be used for
  // services/binaries that MUST run at the lower level of the OS.
  // ATTENTION: USE THIS KIND ONLY WHEN STRICTLY NECESSARY.
  OS = 1,
}

struct Service {
  1: packages.Package svc;
  2: optional packages.Package config_generator;
  3: optional ServiceType svc_type;
} (rust.exhaustive, rust.ord)
