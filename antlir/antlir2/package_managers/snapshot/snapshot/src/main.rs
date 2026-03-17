/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;

use anyhow::Result;
use clap::Parser;
use json_arg::Json;
use tracing::error;
use tracing_subscriber::prelude::*;

mod snapshot;
use snapshot::storage::StorageConfig;

#[derive(Debug, Parser)]
struct Args {
    #[clap(short, long, default_value_t=1, action = clap::ArgAction::Count)]
    verbose: u8,
    #[clap(long)]
    log: Option<PathBuf>,
    #[clap(long)]
    out: PathBuf,
    #[clap(long)]
    storage: Json<StorageConfig>,
    #[clap(subcommand)]
    sub: Sub,
}

#[derive(Debug, Parser)]
enum Sub {
    /// Snapshot metadata tree
    Metadata(snapshot::metadata::Metadata),
    /// Check blob status in storage (missing or expiring soon)
    BlobStatus(snapshot::blob_status::CheckBlobStatus),
    /// Download missing packages and upload them to storage
    Packages(snapshot::packages::Packages),
}

#[fbinit::main]
async fn main(fb: fbinit::FacebookInit) -> ExitCode {
    if let Err(e) = do_main(fb).await {
        error!("{e:#?}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

async fn do_main(fb: fbinit::FacebookInit) -> Result<()> {
    let args = Args::parse();
    let stderr_level = match args.verbose {
        0 => tracing_subscriber::filter::LevelFilter::ERROR,
        1 => tracing_subscriber::filter::LevelFilter::WARN,
        2 => tracing_subscriber::filter::LevelFilter::INFO,
        3 => tracing_subscriber::filter::LevelFilter::DEBUG,
        _ => tracing_subscriber::filter::LevelFilter::TRACE,
    };
    let stderr_layer = tracing_subscriber::fmt::layer().with_filter(stderr_level);
    let file_layer = if let Some(log) = &args.log {
        let file = File::create(log)?;
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

    let storage = args.storage.into_inner().build(fb)?;
    let outfile = BufWriter::new(stdio_path::create(&args.out)?);
    match args.sub {
        Sub::Metadata(metadata) => {
            let out = metadata.run(storage).await?;
            serde_json::to_writer(outfile, &out)?;
        }
        Sub::BlobStatus(blob_status) => {
            let out = blob_status.run(storage).await?;
            serde_json::to_writer(outfile, &out)?;
        }
        Sub::Packages(packages) => {
            let out = packages.run(storage).await?;
            serde_json::to_writer(outfile, &out)?;
        }
    }
    Ok(())
}
