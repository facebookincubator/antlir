/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

namespace cpp2 metalos.host_configs
namespace py3 metalos.host_configs
// @oss-disable: namespace go metalos.host_configs.api

include "common/fb303/if/fb303.thrift"
include "metalos/host_configs/boot_config.thrift"
include "metalos/host_configs/packages.thrift"
include "metalos/host_configs/runtime_config.thrift"

struct UpdateStageResponse {
  // Full set of packages that were downloaded (or already present) as a result
  // of this operation
  1: list<packages.PackageStatus> packages;
} (rust.exhaustive)

/// Currently, the only thing that can go wrong while trying to stage an update
/// is one or more packages failing to download
exception UpdateStageError {
  1: list<packages.PackageStatus> packages;
  2: string message;
} (rust.exhaustive)

// Requests to stage or commit an online update both include the full
// RuntimeConfig, so that clients don't have to keep around any state as long as
// they can recompute the config.
struct OnlineUpdateRequest {
  1: runtime_config.RuntimeConfig runtime_config;
} (rust.exhaustive)

struct OnlineUpdateCommitResponse {
  /// Full set of services that were acted on as a result of this operation
  /// (started/stopped/updated/already-correct)
  1: list<ServiceResponse> services;
} (rust.exhaustive)

struct ServiceResponse {
  1: runtime_config.Service svc;
  2: ServiceOperation operation;
  3: ServiceStatus status;
} (rust.exhaustive)

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

exception OnlineUpdateCommitError {
  1: OnlineUpdateCommitErrorCode code;
  2: string message;
  /// Status of each service
  3: list<ServiceResponse> services;
} (rust.exhaustive)

enum OnlineUpdateCommitErrorCode {
  OTHER = 1,
  // Tried to commit a config that was not previously staged
  NOT_STAGED = 2,
}

struct OfflineUpdateRequest {
  1: boot_config.BootConfig boot_config;
} (rust.exhaustive)

exception OfflineUpdateCommitError {
  1: OfflineUpdateCommitErrorCode code;
  2: string message;
} (rust.exhaustive)

enum OfflineUpdateCommitErrorCode {
  OTHER = 1,
  // Tried to commit a config that was not previously staged
  NOT_STAGED = 2,
  // Kernel could not be loaded
  KEXEC_LOAD = 3,
  // Kexec exec failed for some reason
  KEXEC_EXEC = 4,
}

// This thrift service is primarily (read: exclusively) exposed as subcommands
// of the `metalctl` binary that more or-less match the methods defined here.
service Metalctl extends fb303.FacebookService {
  // Prepare an online update to change the running versions of native
  // services.
  // Corresponds to `metalctl online-update stage`
  UpdateStageResponse online_update_stage(1: OnlineUpdateRequest req) throws (
    1: UpdateStageError e,
  );

  // Commit a previously-prepared online update.
  // Corresponds to `metalctl online-update commit`
  OnlineUpdateCommitResponse online_update_commit(
    1: OnlineUpdateRequest req,
  ) throws (1: OnlineUpdateCommitError e);

  // Prepare an offline update to change the host's BootConfig.  Corresponds to
  // `metalctl offline-update stage`
  UpdateStageResponse offline_update_stage(1: OfflineUpdateRequest req) throws (
    1: UpdateStageError e,
  );

  // Commit a previously-prepared offline update.
  // Corresponds to `metalctl offline-update commit`
  void offline_update_commit(1: OfflineUpdateRequest req) throws (
    1: OfflineUpdateCommitError e,
  );
}
