/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;

use anyhow::Result;
use buck_query::buck_query;
use serde::Deserialize;

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

    println!("<OssOnly>Rustdoc links coming soon</OssOnly>");
    Ok(())
}
