/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::unshare;
use antlir2_isolate::IsolationContext;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    layer: PathBuf,
    #[clap(flatten)]
    out: Out,
    #[clap(last(true))]
    command: String,
}

#[derive(Debug, Parser)]
struct Out {
    #[clap(long)]
    out: PathBuf,

    #[clap(long)]
    dir: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    antlir2_rootless::unshare_new_userns().context("while setting up userns")?;

    let mut builder = IsolationContext::builder(&args.layer);
    builder.ephemeral(false);
    builder.platform([
        #[cfg(facebook)]
        "/usr/local/fbcode",
        #[cfg(facebook)]
        "/mnt/gvfs",
    ]);
    let cwd = std::env::current_dir()?;
    builder
        .inputs((
            Path::new("/__genrule_in_image__/working_directory"),
            cwd.as_path(),
        ))
        .working_directory(Path::new("/__genrule_in_image__/working_directory"))
        .tmpfs(Path::new("/tmp"));

    if args.out.dir {
        std::fs::create_dir_all(&args.out.out)?;
        builder
            .outputs((
                Path::new("/__genrule_in_image__/out"),
                args.out.out.as_path(),
            ))
            .setenv(("OUT", "/__genrule_in_image__/out"));
    } else {
        std::fs::File::create(&args.out.out)?;
        builder
            // some tools might uncontrollably attempt to put temporary files
            // next to the output, so make it a tmpfs
            .tmpfs(Path::new("/__genrule_in_image__/out"))
            .outputs((
                Path::new("/__genrule_in_image__/out/single_file"),
                args.out.out.as_path(),
            ))
            .setenv(("OUT", "/__genrule_in_image__/out/single_file"));
    }

    let isol = unshare(builder.build())?;
    let out = isol
        .command("bash")?
        .arg("-e")
        .arg("-c")
        .arg(&args.command)
        .spawn()?
        .wait()?;
    ensure!(out.success(), "command failed");

    Ok(())
}
