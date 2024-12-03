/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use antlir2_isolate::unshare;
use antlir2_isolate::IsolationContext;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    root: PathBuf,
    #[clap(long)]
    env: Option<JsonFile<BTreeMap<String, Vec<String>>>>,
    #[clap(required(true), trailing_var_arg(true), allow_hyphen_values(true))]
    command: Vec<String>,
}

fn main() -> Result<()> {
    let mut args = Args::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::TRACE)
        .init();

    antlir2_rootless::unshare_new_userns().context("while setting up userns")?;

    let mut builder = IsolationContext::builder(&args.root);
    builder.ephemeral(true);
    #[cfg(facebook)]
    builder.platform(["/usr/local/fbcode", "/mnt/gvfs"]);
    let cwd = std::env::current_dir()?;

    // We need to bind mount buck-out into the target layer. Since we're
    // running as part of a build our cwd should be inside buck-out, so find
    // the shortest cwd path prefix that doesn't exist in the layer and bind
    // mount that in.
    let cwd_vec = cwd.components().collect::<Vec<_>>();
    if cwd_vec.len() > 1 {
        let layer_root = fs::canonicalize(&args.root)?;
        for i in 1..=(cwd_vec.len() - 1) {
            let cwd_prefix = cwd_vec[1..=i].iter().collect::<PathBuf>();
            if !layer_root.join(&cwd_prefix).exists() {
                builder.outputs(Path::new(&Component::RootDir).join(cwd_prefix));
                break;
            }
        }
    }

    if let Some(env) = args.env.map(JsonFile::into_inner) {
        for (k, mut v) in env {
            ensure!(
                v.len() == 1,
                "env var '{k}' expanded to multiple values: {v:#?}"
            );
            builder.setenv((k, v.remove(0)));
        }
    }

    builder
        .working_directory(cwd.as_path())
        .tmpfs(Path::new("/tmp"))
        // TODO(vmagro): make this a devtmpfs after resolving permissions issues
        .tmpfs(Path::new("/dev"));

    let isol = unshare(builder.build())?;
    let mut cmd = isol.command(args.command.remove(0))?;
    cmd.args(args.command);
    sleep(Duration::from_secs(0));
    let out = cmd
        .spawn()
        .context(format!("spawn() failed for {:#?}", cmd))?
        .wait()
        .context(format!("wait() failed for {:#?}", cmd))?;
    ensure!(out.success(), "command failed");

    Ok(())
}
