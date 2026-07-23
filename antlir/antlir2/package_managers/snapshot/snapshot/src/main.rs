/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use tracing::error;
use tracing_subscriber::prelude::*;

mod snapshot;

#[derive(Debug, Parser)]
struct Args {
    #[clap(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    #[clap(long)]
    log: Option<PathBuf>,
    #[clap(subcommand)]
    sub: Sub,
}

#[derive(Debug, Parser)]
enum Sub {
    /// Snapshot a metadata tree directly. Lower-level building block;
    /// most users want `snapshot` instead.
    Metadata(snapshot::metadata::Metadata),
    /// Check blob status in storage. Lower-level building block.
    BlobStatus(snapshot::blob_status::CheckBlobStatus),
    /// Download missing packages and upload them to storage. Lower-level
    /// building block.
    Packages(snapshot::packages::Packages),
    /// Snapshot one or more Buck target patterns end-to-end and rewrite
    /// their BUCK files to use the stored snapshot.
    Snapshot(snapshot::snapshot::Snapshot),
}

#[fbinit::main]
async fn main(fb: fbinit::FacebookInit) -> ExitCode {
    if let Err(e) = do_main(fb).await {
        error!("{e:#}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

async fn do_main(fb: fbinit::FacebookInit) -> Result<()> {
    let args = Args::parse();
    let stderr_level = match args.verbose {
        0 => tracing_subscriber::filter::LevelFilter::WARN,
        1 => tracing_subscriber::filter::LevelFilter::INFO,
        2 => tracing_subscriber::filter::LevelFilter::DEBUG,
        _ => tracing_subscriber::filter::LevelFilter::TRACE,
    };
    // Use the indicatif-aware writer so that tracing logs are printed above
    // the progress bars instead of corrupting them. The writer detects whether
    // stderr is a TTY and falls back to plain stderr when it is not, ensuring
    // progress bars are hidden in non-interactive environments.
    let stderr_writer = snapshot::progress::tracing_writer();
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(stderr_writer)
        .with_filter(stderr_level);
    let file_layer = if let Some(log) = &args.log {
        // Ensure parent dirs exist for log file
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create log dir {}", parent.display()))?;
        }
        let file = File::create(log)
            .with_context(|| format!("failed to create log file {}", log.display()))?;
        Some(
            tracing_subscriber::fmt::layer()
                .with_writer(Mutex::new(file))
                .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG),
        )
    } else {
        None
    };
    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();

    match args.sub {
        Sub::Metadata(metadata) => metadata.run(fb).await,
        Sub::BlobStatus(blob_status) => blob_status.run(fb).await,
        Sub::Packages(packages) => packages.run(fb).await,
        Sub::Snapshot(snapshot) => snapshot.run(fb).await,
    }
}
