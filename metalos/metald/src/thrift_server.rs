/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use async_trait::async_trait;
use slog::Logger;

use thrift_wrapper::ThriftWrapper;

use metalos_thrift_host_configs::api as thrift_api;
use metalos_thrift_host_configs::api::server::Metalctl;
use metalos_thrift_host_configs::api::services::metalctl::OnlineUpdateCommitExn;
use metalos_thrift_host_configs::api::services::metalctl::OnlineUpdateStageExn;

#[derive(Clone)]
pub struct Metald {
    log: Logger,
}

impl Metald {
    pub fn new(log: Logger) -> Self {
        Self { log }
    }
}

#[async_trait]
impl Metalctl for Metald {
    async fn online_update_stage(
        &self,
        req: thrift_api::OnlineUpdateRequest,
    ) -> Result<thrift_api::UpdateStageResponse, OnlineUpdateStageExn> {
        let runtime_config =
            req.runtime_config
                .try_into()
                .map_err(|e: thrift_wrapper::Error| thrift_api::UpdateStageError {
                    packages: vec![],
                    message: e.to_string(),
                })?;
        crate::update::online::stage(self.log.clone(), runtime_config)
            .await
            .map(|r| r.into())
            .map_err(|e| e.into_thrift().into())
    }

    async fn online_update_commit(
        &self,
        req: thrift_api::OnlineUpdateRequest,
    ) -> Result<thrift_api::OnlineUpdateCommitResponse, OnlineUpdateCommitExn> {
        let runtime_config =
            req.runtime_config
                .try_into()
                .map_err(
                    |e: thrift_wrapper::Error| thrift_api::OnlineUpdateCommitError {
                        code: thrift_api::OnlineUpdateCommitErrorCode::OTHER,
                        message: e.to_string(),
                        services: vec![],
                    },
                )?;
        crate::update::online::commit(self.log.clone(), runtime_config)
            .await
            .map(|r| r.into())
            .map_err(|e| e.into_thrift().into())
    }
}
