/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub trait Worker: Sized {
    // Dispatches and dispatches the async worker
    fn new(name: String, context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self>;
    // Returns the status of the worker:
    // * true means the worker is still running
    // * false means the worker terminated gracefully
    // * An error means the worker crashed
    fn get_status(&mut self) -> anyhow::Result<bool>;
}
