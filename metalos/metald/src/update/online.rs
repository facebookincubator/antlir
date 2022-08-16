/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use metalos_host_configs::api::OnlineUpdateCommitError as CommitError;
use metalos_host_configs::api::OnlineUpdateCommitErrorCode as CommitErrorCode;
use metalos_host_configs::api::OnlineUpdateCommitResponse as CommitResponse;
use metalos_host_configs::api::ServiceOperation;
use metalos_host_configs::api::ServiceResponse;
use metalos_host_configs::api::ServiceStatus;
use metalos_host_configs::api::UpdateStageError;
use metalos_host_configs::api::UpdateStageResponse;
use metalos_host_configs::runtime_config::RuntimeConfig;
use package_download::PackageDownloader;
use service::ServiceSet;
use service::Transaction;
use slog::o;
use slog::trace;
use slog::Logger;
use state::State;
use systemd::Systemd;

/// TODO(T121058957) make this nicer
fn map_stage_err<E>(prefix: &'static str) -> impl Fn(E) -> UpdateStageError
where
    E: std::fmt::Debug,
{
    move |e: E| UpdateStageError {
        message: format!("{}: {:?}", prefix, e),
        // TODO(T111087410): include the list of packages
        packages: vec![],
    }
}

pub(crate) async fn stage<D>(
    log: Logger,
    dl: D,
    runtime_config: RuntimeConfig,
) -> Result<UpdateStageResponse, UpdateStageError>
where
    D: PackageDownloader + Clone,
{
    lifecycle::stage(log.clone(), dl, runtime_config)
        .await
        .map_err(map_stage_err("while staging runtimeconfig"))?;

    // TODO(T111087410): return the list of packages
    Ok(UpdateStageResponse { packages: vec![] })
}

pub(crate) async fn commit(
    log: Logger,
    runtime_config: RuntimeConfig,
) -> Result<CommitResponse, CommitError> {
    let log = log.new(o!("runtime-config" => format!("{:?}", runtime_config)));
    trace!(log, "beginning online-update commit");
    let staged_config = RuntimeConfig::staged()
        .ok()
        .flatten()
        .ok_or_else(|| CommitError {
            code: CommitErrorCode::NotStaged,
            message: "no runtime config is staged yet".to_string(),
            // TODO(T111087410): return the list of services
            services: vec![],
        })?;
    if staged_config != runtime_config {
        // TODO(T121058957) make all these error mappings nicer
        return Err(CommitError {
            code: CommitErrorCode::NotStaged,
            message: format!(
                "{:?} does not match the staged runtime config {:?}",
                runtime_config, staged_config
            ),
            // TODO(T111087410): return the list of services
            services: vec![],
        });
    }

    let sd = Systemd::connect(log.clone())
        .await
        .map_err(|e| CommitError {
            code: CommitErrorCode::Other,
            message: format!("error connecting to systemd: {}", e),
            // TODO(T111087410): return the list of services
            services: vec![],
        })?;

    let next = ServiceSet::new(runtime_config.services.clone());

    let tx = Transaction::with_next(&sd, next)
        .await
        .map_err(|e| CommitError {
            code: CommitErrorCode::Other,
            message: format!("error computing transaction: {}", e),
            // TODO(T111087410): return the list of services
            services: vec![],
        })?;
    tx.commit(log, &sd).await.map_err(|e| CommitError {
        code: CommitErrorCode::Other,
        message: format!("error applying transaction: {}", e),
        // TODO(T111087410): return the list of services
        services: vec![],
    })?;

    let token = runtime_config.save().map_err(|e| CommitError {
        code: CommitErrorCode::Other,
        message: format!("error committing runtime config file: {}", e),
        // TODO(T111087410): return the list of services
        services: vec![],
    })?;
    token.commit().map_err(|e| CommitError {
        code: CommitErrorCode::Other,
        message: format!("error committing runtime config file: {}", e),
        // TODO(T111087410): return the list of services
        services: vec![],
    })?;

    Ok(CommitResponse {
        services: runtime_config
            .services
            .iter()
            .map(|svc| ServiceResponse {
                svc: svc.clone(),
                operation: ServiceOperation::Started,
                status: ServiceStatus::Running,
            })
            .collect(),
    })
}
