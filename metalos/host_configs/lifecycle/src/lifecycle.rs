/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! The lifecycle crate is responsible for managing the various lifecycle stages
//! of [MetalOS Host Configs](metalos_host_configs).
//! This library is capable of staging each of the different levels
//! (Provisioning, Boot and Runtime) configs, as well as committing them on the
//! appropriate transitions.

mod boot_config;
mod runtime_config;
mod stage;

pub use stage::stage;
