/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use metalos_host_configs::boot_config::BootConfig;
use metalos_thrift_host_configs::api::OfflineUpdateCommitError;
use metalos_thrift_host_configs::api::OfflineUpdateRequest;
use metalos_thrift_host_configs::api::UpdateStageError;
use metalos_thrift_host_configs::api::UpdateStageResponse;

pub(super) async fn stage(
    metald: super::MetaldClient,
    boot_config: BootConfig,
) -> Result<UpdateStageResponse, UpdateStageError> {
    Ok(metald
        .offline_update_stage_sync(&OfflineUpdateRequest {
            boot_config: boot_config.into(),
        })
        .await
        .expect("TODO(vmagro) make this error conversion work later in stack"))
}

pub(super) async fn commit(
    metald: super::MetaldClient,
    boot_config: BootConfig,
) -> Result<(), OfflineUpdateCommitError> {
    Ok(metald
        .offline_update_commit(&OfflineUpdateRequest {
            boot_config: boot_config.into(),
        })
        .await
        .expect("TODO(vmagro) make this error conversion work later in stack"))
}
