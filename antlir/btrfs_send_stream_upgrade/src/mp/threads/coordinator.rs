/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct Coordinator<'a> {
    /// The base context
    c_context: Option<SendStreamUpgradeContext<'a>>,
}

impl<'a> Coordinator<'a> {
    pub fn new(context: Option<SendStreamUpgradeContext<'a>>) -> anyhow::Result<Self> {
        Ok(Self { c_context: context })
    }
    pub fn run(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    pub fn take_context(&mut self) -> Option<SendStreamUpgradeContext<'a>> {
        self.c_context.take()
    }
}
