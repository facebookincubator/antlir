/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Command;

use buck_query::buck_query;

#[derive(Deserialize)]
struct Library {
    name: String,
    #[serde(rename = "buck.base_path")]
    base_path: String,
    #[serde(rename = "crate")]
    cr8: Option<String>,
}

impl Library {
    fn intern_link(&self) -> String {
        format!(
            "https://www.internalfb.com/intern/rustdoc/{}:{}/{}/index.html",
            self.base_path,
            self.name,
            self.cr8.as_ref().unwrap_or(&self.name)
        )
    }

    fn public_link(&self) -> String {
        format!(
            "https://facebookincubator.github.io/antlir/rustdoc/{}:{}/{}/index.html",
            self.base_path,
            self.name,
            self.cr8.as_ref().unwrap_or(&self.name)
        )
    }
}

fn main() -> Result<()> {
    let targets: BTreeMap<String, Library> =
        buck_query("kind(rust_library, set(//antlir/... //metalos/...))", true)?;

    println!("---\nid: apis\ntitle: Rustdoc\n---");

    println!("<FbInternalOnly>\n");
    for library in targets.values() {
        println!(
            "[//{}:{}]({})  ",
            library.base_path,
            library.name,
            library.intern_link()
        );
    }
    println!("\n</FbInternalOnly>");

    let cwd = std::env::current_dir().unwrap();
    let cell_root = find_root::find_buck_cell_root(&cwd)?;

    // for oss, we have to build the rust doc targets and put them in the right
    // place
    let out = Command::new("buck")
        .arg("build")
        .arg("--show-full-json-output")
        .args(targets.keys().map(|t| format!("{}#doc", t)))
        .output()
        .context("while 'buck build'ing the doc sites")?;
    let sites: HashMap<String, PathBuf> =
        serde_json::from_slice(&out.stdout).context("while deserializing buck output")?;
    let opts = fs_extra::dir::CopyOptions {
        overwrite: true,
        ..Default::default()
    };
    for (target, out) in sites {
        let dstdir = cell_root.join(format!(
            "antlir/website/build/rustdoc/{}",
            target.trim_start_matches('/').trim_end_matches("#doc")
        ));
        std::fs::create_dir_all(&dstdir).context("while creating destination directory")?;
        fs_extra::dir::copy(&out, &dstdir, &opts)
            .with_context(|| format!("while copying {} -> {}", out.display(), dstdir.display()))?;
    }

    println!("<OssOnly>\n");
    for library in targets.values() {
        println!(
            "[//{}:{}]({})  ",
            library.base_path,
            library.name,
            library.public_link()
        );
    }
    println!("\n</OssOnly>");
    Ok(())
}
