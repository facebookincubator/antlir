/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs

include "metalos/host_configs/packages.thrift"
include "metalos/host_configs/runtime_config.thrift"

// Requests to stage or commit an online update both include the full
// RuntimeConfig, so that clients don't have to keep around any state as long as
// they can recompute the config.
struct OnlineUpdateRequest {
  1: runtime_config.RuntimeConfig runtime_config;
} (rust.exhaustive)

struct OnlineUpdateStageResponse {
  // Full set of packages that were downloaded (or already present) as a result
  // of this operation
  1: map<packages.PackageId, packages.Status> packages;
} (rust.exhaustive)

/// Currently, the only thing that can go wrong while trying to stage an online
/// update is one or more packages failing to download
exception OnlineUpdateStageError {
  1: map<packages.PackageId, packages.Status> packages;
}

struct OnlineUpdateCommitResponse {
  /// Full set of services that were acted on as a result of this operation
  /// (started/stopped/updated/already-correct)
  1: map<packages.Service, runtime_config.ServiceResponse> services;
} (rust.exhaustive)

exception OnlineUpdateCommitError {
  1: OnlineUpdateCommitErrorCode code;
  2: string error;
  /// Status of each service
  3: list<runtime_config.ServiceResponse> services;
}

enum OnlineUpdateCommitErrorCode {
  OTHER = 1,
  // Tried to commit a config that was not previously staged
  NOT_STAGED = 2,
}

// TODO(T115253909) Offline updates will also be supported, but online is more
// important now, since an offline update can be accomplished by reprovisioning

// This thrift service is primarily (read: exclusively) exposed as subcommands
// of the `metalctl` binary that more or-less match the methods defined here.
service Metalctl {
  // Prepare an online update to change the running versions of native
  // services.
  // Corresponds to `metalctl online-update stage`
  OnlineUpdateStageResponse online_update_stage(
    1: OnlineUpdateRequest req,
  ) throws (1: OnlineUpdateStageError e);

  // Commit a previously-prepared online update.
  // Corresponds to `metalctl online-update commit`
  OnlineUpdateCommitResponse online_update_commit(
    1: OnlineUpdateRequest req,
  ) throws (1: OnlineUpdateCommitError e);
}
