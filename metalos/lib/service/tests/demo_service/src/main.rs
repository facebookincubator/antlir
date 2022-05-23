/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Simple service demo that illustrates the lifecycle of native services in
//! MetalOS. This service simply appends its current version into each of
//! {Runtime,State,Cache,Logs,Configuration}Directory.
//! metalos/lib/service unit tests will check those files to show that service
//! containers can be started/stopped and updated/downgraded in a safe manner.
//! MetalOS is not concerned with forwards/backwards compatibility of the data
//! in these state directories, that is up to the service itself and is
//! consequently not tested here.

use anyhow::{Context, Result};
use nix::sys::utsname::uname;
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::path::PathBuf;
use structopt::StructOpt;

#[cfg(facebook)]
use facebook_checks::do_facebook_checks;

#[derive(Debug, StructOpt)]
struct Opts {
    #[structopt(long, env = "RUNTIME_DIRECTORY")]
    run: PathBuf,
    #[structopt(long, env = "STATE_DIRECTORY")]
    state: PathBuf,
    #[structopt(long, env = "CACHE_DIRECTORY")]
    cache: PathBuf,
    #[structopt(long, env = "LOGS_DIRECTORY")]
    logs: PathBuf,
}

fn main() -> Result<()> {
    let opts = Opts::from_args();

    // the only guarantee we should check here is that the runtime directory is
    // always empty when the service starts
    assert_eq!(
        0,
        std::fs::read_dir(&opts.run)
            .context("while reading run directory")?
            .count(),
        "runtime directory is not initially empty"
    );

    let uts = uname();
    // the generator generates this dropin to add this to the environment
    assert_eq!(
        uts.release(),
        std::env::var("GENERATOR_KERNEL_VERSION").unwrap()
    );

    #[cfg(facebook)]
    do_facebook_checks()?;

    // for convenience, we use the same binary for every "version" of the demo
    // service, but the METALOS_VERSION environment variable can be used to
    // simulate different versions of the native service
    let current_version = std::env::var("METALOS_VERSION").unwrap();
    println!("logging {} to version file", current_version);

    for dir in &[opts.state, opts.cache, opts.logs] {
        let path = dir.join("version");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("while writing version under {}", dir.display()))?;
        println!(
            "logging {} to version file {}",
            current_version,
            path.display()
        );
        writeln!(f, "{}", current_version)?;
    }

    Ok(())
}
