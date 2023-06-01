/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::prelude::*;

mod integrity;
mod names;

#[derive(Parser)]
enum Args {
    Names(names::Names),
    Integrity(integrity::Integrity),
}

fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::Layer::default()
                .event_format(
                    tracing_glog::Glog::default()
                        .with_span_context(true)
                        .with_timer(tracing_glog::LocalTime::default()),
                )
                .fmt_fields(tracing_glog::GlogFields::default()),
        )
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    match args {
        Args::Names(names) => names.run(),
        Args::Integrity(integrity) => integrity.run(),
    }
}
