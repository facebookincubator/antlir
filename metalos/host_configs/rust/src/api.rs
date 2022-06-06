/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::boot_config;
use crate::packages;
use crate::runtime_config;

use thrift_wrapper::ThriftWrapper;

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::ServiceResponse)]
pub struct ServiceResponse {
    pub svc: runtime_config::Service,
    pub operation: ServiceOperation,
    pub status: ServiceStatus,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::ServiceOperation)]
pub enum ServiceOperation {
    /// Service was started
    Started,
    /// Service was removed
    Stopped,
    /// Service version changed (upgrade or downcrade)
    Changed,
    /// Running service version already matched, this service was not touched
    AlreadyCorrect,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::ServiceStatus)]
pub enum ServiceStatus {
    /// Service was started successfully - it may not be healthy but systemd
    /// reported that it started
    Running,
    /// Service did not start successfully
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::UpdateStageResponse)]
pub struct UpdateStageResponse {
    // Full set of packages that were downloaded (or already present) as a result
    // of this operation
    pub packages: Vec<packages::PackageStatus>,
}

/// Currently, the only thing that can go wrong while trying to stage an online
/// update is one or more packages failing to download
#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::UpdateStageError)]
pub struct UpdateStageError {
    pub packages: Vec<packages::PackageStatus>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::OnlineUpdateRequest)]
pub struct OnlineUpdateRequest {
    pub runtime_config: runtime_config::RuntimeConfig,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::OnlineUpdateCommitErrorCode)]
pub enum OnlineUpdateCommitErrorCode {
    Other,
    /// Tried to commit a config that was not previously staged
    NotStaged,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::OnlineUpdateCommitResponse)]
pub struct OnlineUpdateCommitResponse {
    pub services: Vec<ServiceResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::OnlineUpdateCommitError)]
pub struct OnlineUpdateCommitError {
    pub code: OnlineUpdateCommitErrorCode,
    pub message: String,
    pub services: Vec<ServiceResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::OfflineUpdateRequest)]
pub struct OfflineUpdateRequest {
    pub boot_config: boot_config::BootConfig,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::OfflineUpdateCommitErrorCode)]
pub enum OfflineUpdateCommitErrorCode {
    Other,
    /// Tried to commit a config that was not previously staged
    NotStaged,
    /// Kernel could not be loaded
    KexecLoad,
    // Kexec exec failed for some reason
    KexecExec,
}

#[derive(Debug, Clone, PartialEq, Eq, ThriftWrapper)]
#[thrift(metalos_thrift_host_configs::api::OfflineUpdateCommitError)]
pub struct OfflineUpdateCommitError {
    pub code: OfflineUpdateCommitErrorCode,
    pub message: String,
}