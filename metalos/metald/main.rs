/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use fbinit::FacebookInit;
use metald_lib::start_service;

#[fbinit::main]
async fn main(fb: FacebookInit) -> Result<()> {
    start_service(fb, false).await
}
