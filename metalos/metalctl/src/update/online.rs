/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use metalos_host_configs::runtime_config::RuntimeConfig;
use metalos_thrift_host_configs::api::OnlineUpdateCommitError;
use metalos_thrift_host_configs::api::OnlineUpdateCommitResponse;
use metalos_thrift_host_configs::api::OnlineUpdateRequest;
use metalos_thrift_host_configs::api::UpdateStageError;
use metalos_thrift_host_configs::api::UpdateStageResponse;
use slog::Logger;

pub(super) async fn stage(
    _log: Logger,
    metald: super::MetaldClient,
    _fb: fbinit::FacebookInit,
    runtime_config: RuntimeConfig,
) -> Result<UpdateStageResponse, UpdateStageError> {
    Ok(metald
        .online_update_stage(&OnlineUpdateRequest {
            runtime_config: runtime_config.into(),
        })
        .await
        .expect("TODO(vmagro) make this error conversion work later in stack"))
}

pub(super) async fn commit(
    _log: Logger,
    metald: super::MetaldClient,
    _fb: fbinit::FacebookInit,
    runtime_config: RuntimeConfig,
) -> Result<OnlineUpdateCommitResponse, OnlineUpdateCommitError> {
    Ok(metald
        .online_update_commit(&OnlineUpdateRequest {
            runtime_config: runtime_config.into(),
        })
        .await
        .expect("TODO(vmagro) make this error conversion work later in stack"))
}
