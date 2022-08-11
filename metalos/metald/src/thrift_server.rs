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
use identity::Identity;
use metalos_thrift_host_configs::api as thrift_api;
use metalos_thrift_host_configs::api::server::Metalctl;
use metalos_thrift_host_configs::api::services::metalctl::OnlineUpdateCommitExn;
use metalos_thrift_host_configs::api::services::metalctl::OnlineUpdateStageExn;
use package_download::DefaultDownloader;
use slog::Logger;
use srserver::RequestContext;
use thrift_wrapper::ThriftWrapper;

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

    async fn online_update_stage(
        &self,
        req_ctxt: &RequestContext,
        req: thrift_api::OnlineUpdateRequest,
    ) -> Result<thrift_api::UpdateStageResponse, OnlineUpdateStageExn> {
        match self.check_identity(req_ctxt) {
            Ok(()) => (),
            Err(error) => {
                return Err(OnlineUpdateStageExn::e(thrift_api::UpdateStageError {
                    message: error.to_string(),
                    ..Default::default()
                }));
            }
        };
        let runtime_config =
            req.runtime_config
                .try_into()
                .map_err(|e: thrift_wrapper::Error| thrift_api::UpdateStageError {
                    packages: vec![],
                    message: e.to_string(),
                })?;
        crate::update::online::stage(self.log.clone(), self.dl.clone(), runtime_config)
            .await
            .map(|r| r.into())
            .map_err(|e| e.into_thrift().into())
    }

    async fn online_update_commit(
        &self,
        req_ctxt: &RequestContext,
        req: thrift_api::OnlineUpdateRequest,
    ) -> Result<thrift_api::OnlineUpdateCommitResponse, OnlineUpdateCommitExn> {
        match self.check_identity(req_ctxt) {
            Ok(()) => (),
            Err(error) => {
                return Err(OnlineUpdateCommitExn::e(
                    thrift_api::OnlineUpdateCommitError {
                        message: error.to_string(),
                        ..Default::default()
                    },
                ));
            }
        };
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
