/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(exact_size_is_empty)]

use std::collections::HashMap;
use std::path::Path;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use buck_label::Label;
use buck_version::BuckVersion;
use clap::Parser;
use itertools::Itertools;
use targets_and_outputs::Metadata;
use targets_and_outputs::TargetsAndOutputs;

/// Transform a flattened sequence of target and output location pairs,
/// following the pattern [<targetA>, <outputA>, <targetB>, <outputB>, ...],
/// into a mapping from target name to output path.
///
/// Note that this is only _required_ on buck1, but until we can reasonably use
/// only buck2 (or pay the cost of divergence) we use this indirection method on
/// buck2 as well.
#[derive(Parser)]
struct Args {
    #[clap(long, value_enum)]
    buck_version: BuckVersion,
    #[clap(long)]
    delimiter: String,
    /// The name of the cell that should be assumed for unqualified targets
    #[clap(long)]
    default_cell: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let map = std::io::read_to_string(std::io::stdin()).context("while reading stdin")?;
    let targets_and_outputs = serialize(args, &map)?;
    serde_json::to_writer_pretty(std::io::stdout(), &targets_and_outputs)
        .context("while serializing")?;
    println!();
    Ok(())
}

fn serialize<'a>(args: Args, map: &'a str) -> Result<TargetsAndOutputs<'a>> {
    let mut iter = map.split(&args.delimiter).tuples();
    let mut map = HashMap::new();
    for (label, output) in iter.by_ref() {
        // on buck1, target labels will likely be missing the default cell
        // qualifier, so let's add it
        let label = if label.starts_with("//") {
            format!("{}{}", args.default_cell, label)
        } else {
            label.to_owned()
        };

        let label = Label::new(label)?;
        map.insert(label, Path::new(output).into());
    }
    let remaining = iter.into_buffer();
    ensure!(
        remaining.is_empty(),
        "there were elements left over after splitting: {:?}",
        remaining,
    );
    Ok(TargetsAndOutputs::new(
        Metadata::new(args.buck_version, args.default_cell),
        map,
    ))
}

#[cfg(test)]
mod tests {
    use buck_label::Label;

    use super::*;

    #[test]
    fn adds_default_cell() {
        let tao = serialize(
            Args {
                buck_version: BuckVersion::Two,
                delimiter: "💩".into(),
                default_cell: "🦀".into(),
            },
            "some//fully:qualified💩path/to/qual💩//some/unqualified:target💩path/to/unqual",
        )
        .expect("failed to serialize");
        assert_eq!(2, tao.len());
        assert_eq!(
            Path::new("path/to/qual"),
            tao.path(&Label::new("some//fully:qualified").expect("valid label"))
                .expect("this target exists")
        );
        assert_eq!(
            Path::new("path/to/unqual"),
            tao.path(&Label::new("🦀//some/unqualified:target").expect("valid label"))
                .expect("this target exists")
        );
    }

    #[test]
    fn path_with_space() {
        let tao = serialize(
            Args {
                buck_version: BuckVersion::Two,
                delimiter: "💩".into(),
                default_cell: "🦀".into(),
            },
            "//target:name💩/path/with space",
        )
        .expect("failed to serialize");
        assert_eq!(1, tao.len());
        assert_eq!(
            Path::new("/path/with space"),
            tao.path(&Label::new("🦀//target:name").expect("valid label"))
                .expect("this target exists")
        );
    }

    #[test]
    fn incomplete_input() {
        assert!(
            serialize(
                Args {
                    buck_version: BuckVersion::Two,
                    delimiter: "💩".into(),
                    default_cell: "🦀".into(),
                },
                "//target:name💩/path/with space💩partial",
            )
            .is_err()
        );
    }
}
