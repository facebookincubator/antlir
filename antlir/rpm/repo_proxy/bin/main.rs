/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use tracing_subscriber::prelude::*;

#[derive(Parser, Debug)]
struct Args {
    /// Path to serialized map of repoid -> [repo_proxy::RpmRepo]
    #[clap(long)]
    repos_json: PathBuf,
    #[clap(long, help = "bind to UNIX socket at this path")]
    bind: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
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
        .with(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive("repo_proxy=trace".parse().expect("this is always valid"))
                .from_env()
                .context("while building tracing filter")?,
        )
        .init();

    let args = Args::parse();

    let config_bytes = std::fs::read(&args.repos_json)
        .with_context(|| format!("while reading {}", args.repos_json.display()))?;

    let rpm_repos = serde_json::from_slice(&config_bytes)
        .with_context(|| format!("while parsing {}", args.repos_json.display()))?;

    repo_proxy::serve(repo_proxy::Config::new(
        rpm_repos,
        repo_proxy::Bind::Path(args.bind),
        None,
    ))
    .await
}
