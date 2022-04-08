/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs

// @oss-disable: include "metalos/host_configs/facebook/proxy/if/deployment_specific.thrift"

include "metalos/host_configs/packages.thrift"

// Describes the complete set of software that should be running on a host, as
// well as any config data that must change during a single boot cycle (not
// requiring a reboot).
struct RuntimeConfig {
  // @oss-disable: 1: deployment_specific.DeploymentRuntimeConfig deployment_specific;
  // The complete set of services that should be running on a host after this
  // runtime config is committed.
  2: list<packages.Service> services;
} (rust.exhaustive)

struct ServiceResponse {
  1: ServiceOperation operation;
  2: ServiceStatus status;
}

enum ServiceOperation {
  // Service was started
  STARTED = 1,
  // Service was removed
  STOPPED = 2,
  // Service version changed (upgrade or downcrade)
  CHANGED = 3,
  // Running service version already matched, this service was not touched
  ALREADY_CORRECT = 4,
}

enum ServiceStatus {
  // Service was started successfully - it may not be healthy but systemd
  // reported that it started
  RUNNING = 1,
  // Service did not start successfully
  FAILED = 2,
}
