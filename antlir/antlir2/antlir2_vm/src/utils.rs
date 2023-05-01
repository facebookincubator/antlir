/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::process::Command;

use tracing::debug;
use tracing::trace;

/// Log the command being executed, unless it can't be decoded.
pub(crate) fn log_command(command: &mut Command) -> &mut Command {
    let program = match command.get_program().to_str() {
        Some(s) => s,
        None => {
            debug!("The command is not valid Unicode. Skip logging.");
            return command;
        }
    };
    let args: Option<Vec<&str>> = command.get_args().map(|x| x.to_str()).collect();
    match args {
        Some(args) => trace!("Executing command: {} {}", program, args.join(" ")),
        None => debug!("The command is not valid Unicode. Skip logging."),
    };
    command
}
