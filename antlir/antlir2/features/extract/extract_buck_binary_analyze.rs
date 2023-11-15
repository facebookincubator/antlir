/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::path::PathBuf;

use antlir2_compile::Arch;
use anyhow::Result;
use clap::Parser;
use extract::so_dependencies;
use extract_buck_binary::Lib;
use extract_buck_binary::LibDstPath;
use extract_buck_binary::Manifest;
use tracing::debug;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    src: PathBuf,
    #[clap(long)]
    target_arch: Arch,
    #[clap(long)]
    manifest: PathBuf,
    #[clap(long)]
    libs_dir: PathBuf,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let default_interpreter = extract::default_interpreter(args.target_arch);
    let src = args.src.canonicalize()?;
    let deps = so_dependencies(args.src.to_owned(), None, default_interpreter)?;

    let mut libs = Vec::new();
    for dep in &deps {
        let (src_relpath, dst) =
            match dep.strip_prefix(src.parent().expect("src always has parent")) {
                Ok(relpath) => {
                    debug!(
                        relpath = relpath.display().to_string(),
                        "installing library at path relative to dst"
                    );
                    (
                        Path::new("__relative").join(relpath.strip_prefix("..").unwrap_or(relpath)),
                        LibDstPath::Relative(relpath.to_owned()),
                    )
                }
                Err(_) => {
                    let src_relpath = dep
                        .strip_prefix("/")
                        .expect("non-relative libs are absolute");
                    (src_relpath.to_owned(), LibDstPath::Absolute(dep.to_owned()))
                }
            };

        let copy_path = args.libs_dir.join(&src_relpath);

        std::fs::create_dir_all(copy_path.parent().expect("always has parent"))?;
        std::fs::copy(dep, copy_path)?;

        libs.push(Lib { src_relpath, dst });
    }

    let f = BufWriter::new(File::create(&args.manifest)?);
    serde_json::to_writer_pretty(f, &Manifest(libs))?;

    Ok(())
}
