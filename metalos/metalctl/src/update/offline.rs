/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use slog::{info, Logger};

use super::Subcommand;
use super::Subcommand::{Commit, Stage};

pub(super) async fn run(log: Logger, sub: Subcommand) -> Result<()> {
    match sub {
        Stage(opts) => {
            info!(
                log,
                "I'm staging an offline update! {:?}",
                opts.load::<String>()
            );
            unimplemented!("offline-update stage")
        }
        Commit => {
            unimplemented!("offline-update commit")
        }
    }
}
