/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub trait Worker: Send {
    // Preserve the source of the parent context
    fn preserve_source() -> bool;
    // Preserve the destination of the parent context
    fn preserve_destination() -> bool;
    // Runs a worker on a sendstream context
    fn run_worker(context: SendStreamUpgradeContext) -> anyhow::Result<()>;
}
