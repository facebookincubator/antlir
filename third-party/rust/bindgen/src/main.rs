use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bindgen::builder;
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    header: String,
    #[clap(long)]
    out: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    clang_sys::load()
        .map_err(Error::msg)
        .context("while loading libclang")?;

    let bindings = builder().header(&args.header).generate()?;

    bindings.write_to_file(&args.out)?;
    Ok(())
}
