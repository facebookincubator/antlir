/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(target_os = "linux")]
mod nspawn;

#[cfg(target_os = "linux")]
pub use nspawn::nspawn as isolate;
#[cfg(target_os = "linux")]
pub use nspawn::IsolatedContext;

#[cfg(not(target_os = "linux"))]
compile_error!("only supported on linux");
