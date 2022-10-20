/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Generate modules.bzl in this same directory to provide the Rust megacrate
//! with structured information about the dependencies.

#![feature(exit_status_error)]
use std::collections::BTreeMap;
use std::fmt::Write;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize)]
struct Labels {
    labels: Vec<String>,
}

#[derive(Deserialize)]
struct ModuleDetails {
    rust_crate: String,
    module: String,
}

fn gen_modules_bzl() -> Result<String> {
    let out = Command::new("buck2")
        .arg("query")
        .arg("--output-attributes=labels")
        .arg("attrfilter(labels, 'antlir-rust-extension', set('//antlir/...'))")
        .arg("--reuse-current-config")
        .output()
        .context("buck query failed")?;
    out.status.exit_ok().context("buck query failed")?;
    let targets: BTreeMap<String, Labels> =
        serde_json::from_slice(&out.stdout).context("while parsing buck query output")?;
    let target_list: Vec<&str> = targets.keys().map(String::as_str).collect();
    let mut bzl = concat!("# @", "generated\n").to_string();
    writeln!(
        bzl,
        "extension_rust_targets = {}",
        serde_starlark::to_string_pretty(&target_list).context("while serializing target list")?
    )?;

    let mut target_modules = BTreeMap::new();
    for labels in targets.into_values() {
        let detail_json = labels
            .labels
            .into_iter()
            .filter_map(|l| l.strip_prefix("antlir-rust-extension=").map(str::to_string))
            .next()
            .context("missing antlir-rust-extension label")?;
        let details: ModuleDetails = serde_json::from_str(&detail_json)
            .with_context(|| format!("while parsing {}", detail_json))?;
        target_modules.insert(details.rust_crate, details.module);
    }
    writeln!(
        bzl,
        "extension_modules = {}",
        serde_starlark::to_string_pretty(&target_modules)
            .context("while serializing target modules map")?
    )?;

    Ok(bzl)
}

fn main() -> Result<()> {
    println!("{}", gen_modules_bzl()?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn modules_bzl_uptodate() {
        assert_eq!(
            gen_modules_bzl().expect("generating modules.bzl failed"),
            include_str!("modules.bzl"),
        );
    }
}
