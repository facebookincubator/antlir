/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;

use antlir2_compile::Arch;
use anyhow::Context;
use anyhow::Result;
use apt::AptItem;
use apt::DriverContext;
use buck_label::Label;
use clap::Parser;
use json_arg::JsonFile;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    label: Label,
    #[clap(long)]
    build_appliance: PathBuf,
    #[clap(long)]
    dpkg_status: Option<PathBuf>,
    #[clap(long)]
    dpkg_status_out: Option<PathBuf>,
    #[clap(long)]
    archive_dir: PathBuf,
    #[clap(long)]
    target_arch: Arch,
    #[clap(long)]
    items: JsonFile<Vec<AptItem>>,
    #[clap(long)]
    resolve_cmd: Vec<String>,
    #[clap(long)]
    out: PathBuf,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let args = Args::parse();

    antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
    antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;

    let dpkg_status_content = args
        .dpkg_status
        .as_ref()
        .map(|path| {
            let file = File::open(path).context("while opening dpkg status file")?;
            let mut reader = BufReader::new(file);
            let mut content = String::new();
            reader
                .read_to_string(&mut content)
                .context("while reading dpkg status file")?;
            Ok::<_, anyhow::Error>(content)
        })
        .transpose()?;

    let apt = apt::Apt {
        items: args.items.into_inner(),
        driver_cmd: args.resolve_cmd,
    };
    let (tx, dpkg_status_out) = apt
        .plan(DriverContext::plan(
            args.label,
            args.build_appliance,
            args.archive_dir,
            args.target_arch,
            dpkg_status_content,
        ))
        .context("while planning transaction")?;

    let out = BufWriter::new(File::create(&args.out).context("while creating output file")?);
    serde_json::to_writer(out, &tx).context("while serializing plan")?;

    if let Some(out_path) = &args.dpkg_status_out {
        let status = dpkg_status_out.as_deref().unwrap_or("");
        let file = File::create(out_path).context("while creating dpkg status output file")?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(status.as_bytes())
            .context("while writing dpkg status output")?;
    }

    Ok(())
}
