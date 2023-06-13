/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(io_error_other)]

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::io::Error;
use std::path::PathBuf;

use antlir2_users::group::EtcGroup;
use antlir2_users::passwd::EtcPasswd;
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
    let mut paths_that_exist_in_layer = HashSet::new();

    let layer_userdb: EtcPasswd = std::fs::read_to_string(args.layer.join("etc/passwd"))
        .and_then(|s| s.parse().map_err(Error::other))
        .unwrap_or_else(|_| Default::default());
    let layer_groupdb: EtcGroup = std::fs::read_to_string(args.layer.join("etc/group"))
        .and_then(|s| s.parse().map_err(Error::other))
        .unwrap_or_else(|_| Default::default());
    let parent_userdb: EtcPasswd = std::fs::read_to_string(args.parent.join("etc/passwd"))
        .and_then(|s| s.parse().map_err(Error::other))
        .unwrap_or_else(|_| Default::default());
    let parent_groupdb: EtcGroup = std::fs::read_to_string(args.parent.join("etc/group"))
        .and_then(|s| s.parse().map_err(Error::other))
        .unwrap_or_else(|_| Default::default());

    for fs_entry in WalkDir::new(&args.layer) {
        let fs_entry = fs_entry?;
        if fs_entry.path() == args.layer {
            continue;
        }
        let relpath = fs_entry
            .path()
            .strip_prefix(&args.layer)
            .expect("this must be relative");
        let parent_path = args.parent.join(relpath);
        let entry = Entry::new(fs_entry.path(), &layer_userdb, &layer_groupdb)
            .with_context(|| format!("while building Entry for '{}", relpath.display()))?;
        paths_that_exist_in_layer.insert(relpath.to_path_buf());
        // broken symlinks are allowed
        let parent_exists = if fs_entry.path_is_symlink() {
            std::fs::read_link(&parent_path).is_ok()
        } else {
            parent_path.exists()
        };
        if !parent_exists {
            entries.insert(relpath.to_path_buf(), Diff::Added(entry));
        } else {
            let parent_entry = Entry::new(&parent_path, &parent_userdb, &parent_groupdb)
                .with_context(|| {
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
    for fs_entry in WalkDir::new(&args.parent) {
        let fs_entry = fs_entry?;
        if fs_entry.path() == args.parent {
            continue;
        }
        let relpath = fs_entry
            .path()
            .strip_prefix(&args.parent)
            .expect("this must be relative");
        if !paths_that_exist_in_layer.contains(relpath) {
            let entry = Entry::new(fs_entry.path(), &parent_userdb, &parent_groupdb)
                .with_context(|| format!("while building Entry for '{}'", relpath.display()))?;
            entries.insert(relpath.to_path_buf(), Diff::Removed(entry));
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
                        &toml::to_string_pretty(&diff)
                            .with_context(|| format!("while serializing actual diff: {diff:#?}"))?,
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
