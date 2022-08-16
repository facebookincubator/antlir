/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

include "metalos/host_configs/runtime_config.thrift"

typedef string Uuid

struct ServiceInstance {
  1: runtime_config.Service svc;
  2: Uuid run_uuid;
} (rust.exhaustive)
