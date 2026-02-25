/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::ValueEnum;

#[derive(Debug, Parser)]
pub(crate) struct Decompress {
    input: PathBuf,
    out: PathBuf,
    #[clap(long)]
    format: Format,
}

#[derive(Debug, Clone, ValueEnum)]
enum Format {
    Gz,
    Xz,
}

impl Decompress {
    pub(crate) fn run(&self) -> Result<()> {
        let input = io::BufReader::new(
            std::fs::File::open(&self.input)
                .with_context(|| format!("failed to open {}", self.input.display()))?,
        );
        let mut output = io::BufWriter::new(
            std::fs::File::create(&self.out)
                .with_context(|| format!("failed to create {}", self.out.display()))?,
        );

        match self.format {
            Format::Gz => {
                let mut decoder = flate2::read::GzDecoder::new(input);
                io::copy(&mut decoder, &mut output).context("failed to decompress gzip")?;
            }
            Format::Xz => {
                let mut decoder = liblzma::read::XzDecoder::new(input);
                io::copy(&mut decoder, &mut output).context("failed to decompress xz")?;
            }
        }

        Ok(())
    }
}
