/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::fmt::Display;
use std::thread;
use std::thread::JoinHandle;

use crate::mp::threads::worker::Worker;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct WorkerThread {
    /// The name associated with the worker thread
    wt_name: String,
    /// The join handle to check the status of the worker thread
    wt_status: Option<JoinHandle<anyhow::Result<()>>>,
}

impl Display for WorkerThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl Debug for WorkerThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl WorkerThread {
    // Generates a new worker which will use a new context
    // derived from the given context
    pub fn new<W: Worker>(
        name: String,
        context: &mut SendStreamUpgradeContext,
    ) -> anyhow::Result<Self> {
        let sync_container = match context.ssuc_sync_container {
            Some(ref sync_container) => Some(sync_container.clone()),
            None => anyhow::bail!("Creating new read worker for context without sync container"),
        };
        let new_context = SendStreamUpgradeContext::clone_for_mp_threads(
            W::preserve_source(),
            W::preserve_destination(),
            context.ssuc_logger.clone(),
            context.ssuc_options.clone(),
            context.get_source_version()?,
            context.get_destination_version()?,
            context.get_read_offset(),
            context.get_write_offset(),
            sync_container,
        )?;

        Ok(Self {
            wt_name: name,
            wt_status: Some(thread::spawn(move || W::run_worker(new_context))),
        })
    }
    // Returns the status of the worker:
    // * true means the worker is still running
    // * false means the worker terminated gracefully
    // * An error means the worker crashed
    pub fn get_status(&mut self) -> anyhow::Result<bool> {
        match self.wt_status {
            Some(ref handle) => {
                // Return true if the handle is finished
                if !handle.is_finished() {
                    return Ok(true);
                }
            }
            None => anyhow::bail!(
                "Failed to get status handle in worker thread {}",
                self.wt_name
            ),
        }
        // The thread is done now
        // Remove the join handle and look it up
        let handle = match self.wt_status.take() {
            Some(handle) => handle,
            None => anyhow::bail!(
                "Unexepcted None status handle in worker thread {}",
                self.wt_name
            ),
        };
        match handle.join() {
            Ok(Ok(())) => Ok(false),
            // Normal anyhow error propagation
            Ok(Err(e)) => anyhow::bail!(e),
            // Note: This can happen in case of a panic
            // Just do our best here...
            Err(e) => anyhow::bail!("Thread {} paniced because {:?}", self.wt_name, e),
        }
    }

    fn fmt_internal(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<WorkerThread Name={} Status={:?}/>",
            self.wt_name, self.wt_status
        )
    }
}
