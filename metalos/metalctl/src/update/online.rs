/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use slog::Logger;

use metalos_host_configs::api::{
    OnlineUpdateCommitError, OnlineUpdateCommitResponse, UpdateStageError, UpdateStageResponse,
};
use metalos_host_configs::runtime_config::RuntimeConfig;

/// TODO(T121058957) make this nicer
fn map_stage_err<E>(prefix: &'static str) -> impl Fn(E) -> UpdateStageError
where
    E: std::fmt::Debug,
{
    move |e: E| UpdateStageError {
        message: format!("{}: {:?}", prefix, e),
        ..Default::default()
    }
}

pub(super) async fn stage(
    log: Logger,
    runtime_config: RuntimeConfig,
) -> Result<UpdateStageResponse, UpdateStageError> {
    lifecycle::stage(log.clone(), runtime_config)
        .await
        .map_err(map_stage_err("while staging runtimeconfig"))?;

    // TODO(T111087410): return the list of packages
    Ok(UpdateStageResponse { packages: vec![] })
}

pub(super) async fn commit(
    _log: Logger,
    _runtime_config: RuntimeConfig,
) -> Result<OnlineUpdateCommitResponse, OnlineUpdateCommitError> {
    todo!("in next diff")
}
