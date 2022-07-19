/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use fb303::fb_status;
use fb303::server::FacebookService;
use fb303_core::server::BaseService;
use fb303_core::services::base_service::GetNameExn;
use fb303_core::services::base_service::GetStatusDetailsExn;
use fb303_core::services::base_service::GetStatusExn;
use fbinit::FacebookInit;

use metalos_thrift_host_configs::api::server::Metalctl;
use metalos_thrift_host_configs::api::services::metalctl::OnlineUpdateCommitExn;
use metalos_thrift_host_configs::api::services::metalctl::OnlineUpdateStageExn;
use metalos_thrift_host_configs::api::OnlineUpdateCommitResponse;
use metalos_thrift_host_configs::api::OnlineUpdateRequest;
use metalos_thrift_host_configs::api::UpdateStageResponse;

use async_trait::async_trait;

#[derive(Clone)]
pub struct FacebookServiceImpl;

#[async_trait]
impl BaseService for FacebookServiceImpl {
    async fn getName(&self) -> Result<String, GetNameExn> {
        Ok("Metald API Server".to_string())
    }

    async fn getStatusDetails(&self) -> Result<String, GetStatusDetailsExn> {
        Ok("Alive and running.".to_string())
    }

    async fn getStatus(&self) -> Result<fb_status, GetStatusExn> {
        Ok(fb_status::ALIVE)
    }
}

impl FacebookService for FacebookServiceImpl {}

#[derive(Clone)]
pub struct MetalctlImpl {
    pub fb: FacebookInit,
}

#[async_trait]
impl Metalctl for MetalctlImpl {
    async fn online_update_stage(
        &self,
        _input: OnlineUpdateRequest,
    ) -> Result<UpdateStageResponse, OnlineUpdateStageExn> {
        Ok(UpdateStageResponse {
            ..Default::default()
        })
    }

    async fn online_update_commit(
        &self,
        _input: OnlineUpdateRequest,
    ) -> Result<OnlineUpdateCommitResponse, OnlineUpdateCommitExn> {
        Ok(OnlineUpdateCommitResponse {
            ..Default::default()
        })
    }
}
