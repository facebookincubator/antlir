/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_feature_impl::Feature as _;
use anyhow::Result;
use clap::Parser;
use r#impl::Feature;
use json_arg::Json;
use tracing_subscriber::prelude::*;

#[derive(Parser)]
struct Args {
    feature: Json<r#impl::Feature>,
    #[clap(subcommand)]
    cmd: Cmd,
}

#[derive(Parser)]
enum Cmd {
    Provides,
    Requires,
    Compile {
        #[clap(long)]
        ctx: Json<CompilerContext>,
    },
    Plan {
        #[clap(long)]
        ctx: Json<CompilerContext>,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::Layer::default()
                .with_writer(std::io::stderr)
                .event_format(
                    tracing_glog::Glog::default()
                        .with_span_context(true)
                        .with_timer(tracing_glog::LocalTime::default()),
                )
                .fmt_fields(tracing_glog::GlogFields),
        )
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let feature = args.feature.into_inner();
    match args.cmd {
        Cmd::Provides => {
            serde_json::to_writer_pretty(std::io::stdout(), &Feature::provides(&feature)?)?;
        }
        Cmd::Requires => {
            serde_json::to_writer_pretty(std::io::stdout(), &Feature::requires(&feature)?)?;
        }
        Cmd::Compile { ctx } => {
            Feature::compile(&feature, ctx.as_inner())?;
        }
        Cmd::Plan { ctx } => {
            serde_json::to_writer_pretty(
                std::io::stdout(),
                &Feature::plan(&feature, ctx.as_inner())?,
            )?;
        }
    }
    Ok(())
}
