/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;

use crate::mp::threads::worker::Worker;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct PrefetchWorker {}

unsafe impl Send for PrefetchWorker {}

impl Worker for PrefetchWorker {
    fn preserve_source() -> bool {
        // The prefetcher can get the source
        true
    }
    fn preserve_destination() -> bool {
        // Only the writer can get the destination
        false
    }
    fn run_worker(context: SendStreamUpgradeContext) -> anyhow::Result<()> {
        let mut context = context;
        // Detatch the container from the buffer cache
        let sync_container = context
            .ssuc_sync_container
            .as_mut()
            .context("Prefetching with None container")?;
        let buffer_cache = sync_container
            .take_buffer_cache()
            .context("Prefetching with None buffer cache")?;

        // Run the prefetch loop
        (*buffer_cache).prefetcher_loop(&mut context)?;

        // On our way out, tally up the stats
        context
            .ssuc_sync_container
            .as_ref()
            .context("Prefetching with None container")?
            .rollover_stats(&context.ssuc_stats)?;
        Ok(())
    }
}
