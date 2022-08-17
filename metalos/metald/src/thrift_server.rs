/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;
#[cfg(not(facebook))]
use fbthrift::RequestContext;
use identity::Identity;
use metalos_host_configs::api::Metalctl;
use metalos_host_configs::api::OfflineUpdateCommitError;
use metalos_host_configs::api::OfflineUpdateRequest;
use metalos_host_configs::api::OnlineUpdateCommitError;
use metalos_host_configs::api::OnlineUpdateCommitResponse;
use metalos_host_configs::api::OnlineUpdateRequest;
use metalos_host_configs::api::UpdateStageError;
use metalos_host_configs::api::UpdateStageResponse;
use netos_metald as thrift_api_netos;
use netos_metald::server::MetalctlNetos;
use netos_metald::services::metalctl_netos::DummyMethodExn;
use package_download::DefaultDownloader;
use slog::Logger;
#[cfg(facebook)]
use srserver::RequestContext;

use crate::acl::PermissionsChecker;

const ACL_ACTION: &str = "canDoCyborgJob"; //TODO:T117800273 temporarily, need to decide the correct acl

pub struct Metald<C>
where
    C: PermissionsChecker<Identity = Identity>,
{
    log: Logger,
    dl: DefaultDownloader,
    acl_checker: Arc<C>,
}

impl<C> Clone for Metald<C>
where
    C: PermissionsChecker<Identity = Identity>,
{
    fn clone(&self) -> Self {
        Self {
            log: self.log.clone(),
            dl: self.dl.clone(),
            acl_checker: self.acl_checker.clone(),
        }
    }
}

impl<C> Metald<C>
where
    C: PermissionsChecker<Identity = Identity>,
{
    pub fn new(fb: FacebookInit, log: Logger, acl_checker: Arc<C>) -> Result<Self> {
        let dl = package_download::default_downloader(fb)
            .context("while building default downloader")?;

        Ok(Self {
            log,
            dl,
            acl_checker,
        })
    }

    fn check_identity(&self, req_ctxt: &RequestContext) -> anyhow::Result<()> {
        let id_set = req_ctxt.identities()?;
        let ids: Vec<_> = id_set
            .entries()
            .into_iter()
            .map(identity::ffi::copyIdentity)
            .collect();
        match self
            .acl_checker
            .action_allowed_for_identity(&ids, ACL_ACTION)
        {
            crate::acl::Result::Allowed => Ok(()),
            crate::acl::Result::Denied(d) => Err(Error::msg(format!("{:?}", d))),
        }
    }
}

#[async_trait]
impl<C> Metalctl for Metald<C>
where
    C: PermissionsChecker<Identity = Identity> + 'static,
{
    type RequestContext = RequestContext;

    async fn online_update_stage_sync(
        &self,
        req_ctxt: &RequestContext,
        req: OnlineUpdateRequest,
    ) -> Result<UpdateStageResponse, UpdateStageError> {
        match self.check_identity(req_ctxt) {
            Ok(()) => (),
            Err(error) => {
                return Err(UpdateStageError {
                    message: error.to_string(),
                    ..Default::default()
                });
            }
        };
        crate::update::online::stage(self.log.clone(), self.dl.clone(), req.runtime_config).await
    }

    async fn online_update_commit_sync(
        &self,
        req_ctxt: &RequestContext,
        req: OnlineUpdateRequest,
    ) -> Result<OnlineUpdateCommitResponse, OnlineUpdateCommitError> {
        match self.check_identity(req_ctxt) {
            Ok(()) => (),
            Err(error) => {
                return Err(OnlineUpdateCommitError {
                    message: error.to_string(),
                    ..Default::default()
                });
            }
        };
        crate::update::online::commit(self.log.clone(), req.runtime_config).await
    }

    async fn offline_update_stage_sync(
        &self,
        req_ctxt: &RequestContext,
        req: OfflineUpdateRequest,
    ) -> Result<UpdateStageResponse, UpdateStageError> {
        match self.check_identity(req_ctxt) {
            Ok(()) => (),
            Err(error) => {
                return Err(UpdateStageError {
                    message: error.to_string(),
                    ..Default::default()
                });
            }
        };
        crate::update::offline::stage(self.log.clone(), self.dl.clone(), req.boot_config).await
    }

    async fn offline_update_commit(
        &self,
        req_ctxt: &RequestContext,
        req: OfflineUpdateRequest,
    ) -> Result<(), OfflineUpdateCommitError> {
        match self.check_identity(req_ctxt) {
            Ok(()) => (),
            Err(error) => {
                return Err(OfflineUpdateCommitError {
                    message: error.to_string(),
                    ..Default::default()
                });
            }
        };
        crate::update::offline::commit(self.log.clone(), req.boot_config).await
    }
}

#[async_trait]
impl<C> MetalctlNetos for Metald<C>
where
    C: PermissionsChecker<Identity = Identity> + 'static,
{
    type RequestContext = RequestContext;

    async fn dummy_method(
        &self,
        _req_ctxt: &RequestContext,
        req: thrift_api_netos::DummyMethodRequest,
    ) -> Result<thrift_api_netos::DummyMethodResponse, DummyMethodExn> {
        Ok(thrift_api_netos::DummyMethodResponse {
            message: req.message,
        })
    }
}
