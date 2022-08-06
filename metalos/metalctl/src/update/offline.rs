/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;
use anyhow::Result;
use metalos_host_configs::api::OfflineUpdateCommitError as CommitError;
use metalos_host_configs::api::OfflineUpdateCommitErrorCode as CommitErrorCode;
use metalos_host_configs::api::UpdateStageError as StageError;
use metalos_host_configs::api::UpdateStageResponse as StageResponse;
use metalos_host_configs::boot_config::BootConfig;
use metalos_host_configs::host::HostConfig;
use metalos_kexec::KexecInfo;
use slog::info;
use slog::o;
use slog::trace;
use slog::Logger;
use state::State;

fn map_stage_err<E>(prefix: &'static str) -> impl Fn(E) -> StageError
where
    E: std::fmt::Display,
{
    move |e: E| StageError {
        message: format!("{}: {}", prefix, e),
        // TODO(T111087410): include the list of packages
        packages: vec![],
    }
}

pub(super) async fn stage(
    log: Logger,
    _metald: super::MetaldClient,
    fb: fbinit::FacebookInit,
    boot_config: BootConfig,
) -> Result<StageResponse, StageError> {
    let dl = package_download::default_downloader(fb)
        .map_err(map_stage_err("failed to create PackageDownloader"))?;
    lifecycle::stage(log.clone(), dl, boot_config)
        .await
        .map_err(map_stage_err("while staging BootConfig"))?;
    // TODO(T111087410): return the list of packages
    Ok(StageResponse { packages: vec![] })
}

fn map_commit_err<E>(prefix: &'static str) -> impl Fn(E) -> CommitError
where
    E: std::fmt::Display,
{
    move |e: E| CommitError {
        code: CommitErrorCode::Other,
        message: format!("{}: {}", prefix, e),
    }
}

pub(super) async fn commit(
    log: Logger,
    _metald: super::MetaldClient,
    _fb: fbinit::FacebookInit,
    boot_config: BootConfig,
) -> Result<(), CommitError> {
    let log = log.new(o!("boot-config" => format!("{:?}", boot_config)));
    trace!(log, "beginning offline-update commit");

    let staged_config = BootConfig::staged()
        .ok()
        .flatten()
        .ok_or_else(|| CommitError {
            code: CommitErrorCode::NotStaged,
            message: "no boot config is staged yet".to_string(),
        })?;
    if staged_config != boot_config {
        return Err(CommitError {
            code: CommitErrorCode::NotStaged,
            message: format!(
                "{:?} does not match the staged boot config {:?}",
                boot_config, staged_config
            ),
            ..Default::default()
        });
    }

    // merge the new BootConfig with the full HostConfig
    let mut host_config = HostConfig::current()
        .context("while loading committed HostConfig")
        .map_err(map_commit_err("could not load committed HostConfig"))?
        .ok_or_else(|| CommitError {
            code: CommitErrorCode::Other,
            message: "no committed HostConfig".into(),
        })?;
    host_config.boot_config = boot_config;
    let host_config_token = host_config
        .save()
        .context("while saving merged HostConfig")
        .map_err(map_commit_err("could not save merged HostConfig"))?;
    host_config_token
        .stage()
        .context("while staging merged HostConfig")
        .map_err(map_commit_err("could not stage merged HostConfig"))?;
    info!(log, "marked merged HostConfig as staged");
    // TODO(T121845483): mark this as pre-committed, and have the setup initrd
    // mark it as committed
    host_config_token
        .commit()
        .context("while committing merged HostConfig")
        .map_err(map_commit_err("could not commit merged HostConfig"))?;
    info!(log, "marked merged HostConfig as committed");

    let kexec_info = KexecInfo::new_from_packages(
        &host_config.boot_config.kernel,
        &host_config.boot_config.initrd,
        host_config.boot_config.kernel.cmdline.clone(),
    )
    .map_err(map_commit_err("could not build KexecInfo"))?;

    kexec_info
        .kexec(log)
        .await
        .map_err(map_commit_err("could not kexec"))
}
