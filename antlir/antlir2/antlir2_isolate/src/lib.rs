/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! antlir2_isolate
//! ===============
//!
//! This crate serves to set up an isolated environment in which to perform
//! image compilation. This does not do any of the compilation or deal with
//! subvolume management, it simply prepares an isolation environment with
//! already-existing images.

pub mod sys;

#[derive(Debug, thiserror::Error)]
pub enum Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub use isolate_cfg::InvocationType;
pub use isolate_cfg::IsolationContext;
/// Set up an isolated environment to run a compilation process.
pub use sys::nspawn;
/// Dynamic information about the isolated environment that might be necessary
/// for the image build.
pub use sys::IsolatedContext;
