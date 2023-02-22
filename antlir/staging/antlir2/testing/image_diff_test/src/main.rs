/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use json_arg::TomlFile;
use similar::TextDiff;
use walkdir::WalkDir;

mod entry;
use entry::Entry;
mod diff;
use diff::Diff;
use diff::EntryDiff;
use diff::LayerDiff;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    /// Parent layer
    parent: PathBuf,
    #[clap(long)]
    /// Child layer
    layer: PathBuf,
    #[clap(subcommand)]
    subcommand: Subcommand,
}

#[derive(Parser)]
enum Subcommand {
    /// Print the diff between the two in TOML form
    Print,
    Test {
        #[clap(long)]
        expected: TomlFile<LayerDiff>,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut entries = BTreeMap::new();
    let layer = &args
        .layer
        .canonicalize()
        .context("while looking up realpath of layer")?;
    for fs_entry in WalkDir::new(layer) {
        let fs_entry = fs_entry?;
        let relpath = fs_entry
            .path()
            .strip_prefix(layer)
            .expect("this must be relative");
        let parent_path = args.parent.join(relpath);
        if fs_entry.file_type().is_file() || fs_entry.file_type().is_symlink() {
            let entry = Entry::new(fs_entry.path())
                .with_context(|| format!("while building Entry for '{}", relpath.display()))?;
            if !parent_path.exists() {
                entries.insert(relpath.to_path_buf(), Diff::Added(entry));
            } else {
                let parent_entry = Entry::new(&parent_path).with_context(|| {
                    format!(
                        "while building Entry for parent version of '{}",
                        relpath.display(),
                    )
                })?;
                if parent_entry != entry {
                    entries.insert(
                        relpath.to_path_buf(),
                        Diff::Diff(EntryDiff::new(&parent_entry, &entry)),
                    );
                }
            }
        }
    }
    let diff = LayerDiff(entries);

    match args.subcommand {
        Subcommand::Print => {
            println!(
                "{}",
                toml::to_string_pretty(&diff).context("while serializing")?
            );
        }
        Subcommand::Test { expected } => {
            if &diff != expected.as_inner() {
                println!(
                    "{}",
                    TextDiff::from_lines(
                        &toml::to_string_pretty(expected.as_inner())
                            .context("while (re)serializing expected")?,
                        &toml::to_string_pretty(&diff).context("while serializing actual diff")?,
                    )
                    .unified_diff()
                    .context_radius(3)
                    .header("expected", "actual")
                );
                anyhow::bail!("actual diff did not match expected diff");
            }
        }
    }
    Ok(())
}
