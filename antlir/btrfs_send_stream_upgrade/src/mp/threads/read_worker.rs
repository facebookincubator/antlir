/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::mp::threads::worker::Worker;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct ReadWorker {}

unsafe impl Send for ReadWorker {}

impl Worker for ReadWorker {
    fn preserve_source() -> bool {
        // Only the prefetcher can get the source
        false
    }
    fn preserve_destination() -> bool {
        // Only the writer can get the destination
        false
    }
    fn run_worker(_context: SendStreamUpgradeContext) -> anyhow::Result<()> {
        Ok(())
    }
}
