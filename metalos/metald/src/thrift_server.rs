/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use slog::Logger;

use fbinit::FacebookInit;
use fbwhoami::FbWhoAmI;
use srserver::RequestContext;

use package_download::DefaultDownloader;
use thrift_wrapper::ThriftWrapper;

use metalos_thrift_host_configs::api as thrift_api;
use metalos_thrift_host_configs::api::server::Metalctl;
use metalos_thrift_host_configs::api::services::metalctl::OnlineUpdateCommitExn;
use metalos_thrift_host_configs::api::services::metalctl::OnlineUpdateStageExn;

use acl_checker::AclCheckerService;
use aclchecker::AclChecker;
use aclchecker::AclCheckerError;
use fallback_identity_checker::FallbackIdentityChecker;
use identity::Identity;
use identity::IdentitySet;
use permission_checker::PermissionsChecker;

const ACL_ACTION: &str = "canDoCyborgJob"; //TODO:T117800273 temporarely, need to decide the correct acl
const ALLOWED_IDENTITIES_PATH: [&str; 2] = [
    "usr/lib/metald/metald_allowed_identities",
    "etc/metald/metald_allowed_identities",
];

#[derive(thiserror::Error, Debug)]
pub enum MetalctlImplError {
    #[error("Internal ACL checker error: {0:?}")]
    AclCheckerError(#[from] AclCheckerError),
    #[error("Hostname scheme not found for this device from fbwhoami data")]
    HostnameSchemeNotFound,
}

#[derive(Clone)]
pub struct Metald {
    log: Logger,
    dl: DefaultDownloader,
    pub fallback_identity_checker: FallbackIdentityChecker<AclCheckerService<AclChecker>>,
}

impl Metald {
    pub fn new(fb: FacebookInit, log: Logger) -> Result<Self> {
        let dl = package_download::default_downloader(fb)
            .context("while building default downloader")?;

        let whoami = FbWhoAmI::get()?;
        if let Some(hostname_prefix) = whoami.hostname_scheme.as_ref() {
            let id = Identity::with_machine_tier(hostname_prefix);
            let fb_acl_checker = AclChecker::new(fb, &id)?;
            let acl_checker = AclCheckerService::new(fb_acl_checker, hostname_prefix);
            Ok(Self {
                log,
                dl,
                fallback_identity_checker: FallbackIdentityChecker::new(
                    acl_checker,
                    ALLOWED_IDENTITIES_PATH.iter().map(|&s| s.into()).collect(),
                ),
            })
        } else {
            Err(MetalctlImplError::HostnameSchemeNotFound).map_err(|err| err.into())
        }
    }

    fn check_identity(&self, req_ctxt: &RequestContext) -> anyhow::Result<bool> {
        let ids = req_ctxt.identities()?;
        verify_identity_against_checker(self.fallback_identity_checker.clone(), &ids)
    }
}

fn verify_identity_against_checker<C>(checker: C, ids: &IdentitySet) -> anyhow::Result<bool>
where
    C: PermissionsChecker,
{
    Ok(checker.action_allowed_for_identity(ids, ACL_ACTION)?)
}

#[async_trait]
impl Metalctl for Metald {
    type RequestContext = RequestContext;

    async fn online_update_stage(
        &self,
        req_ctxt: &RequestContext,
        req: thrift_api::OnlineUpdateRequest,
    ) -> Result<thrift_api::UpdateStageResponse, OnlineUpdateStageExn> {
        match self.check_identity(req_ctxt) {
            Ok(_) => (),
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
            Ok(_) => (),
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
