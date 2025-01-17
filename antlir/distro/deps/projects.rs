/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use serde::ser::SerializeTuple;
use serde::ser::Serializer;
use serde::Deserialize;
use serde::Serialize;
use signedsource::sign_with_generated_header;
use signedsource::Comment;

#[derive(Debug, Deserialize)]
#[serde(rename = "struct")]
struct Dep {
    project: String,
    name: String,
}

impl Serialize for Dep {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut t = serializer.serialize_tuple(2)?;
        t.serialize_element(&self.project)?;
        t.serialize_element(&self.name)?;
        t.end()
    }
}

fn output_path() -> Result<PathBuf> {
    let out = Command::new("buck2")
        .arg("root")
        .output()
        .context("while running buck2 root")?;
    ensure!(out.status.success(), "buck2 root failed");
    let stdout = String::from_utf8(out.stdout).context("buck2 root output not utf8")?;
    let root = Path::new(stdout.trim());
    Ok(root.join("antlir/distro/deps/projects.bzl"))
}

fn gen_bzl(deps: &[Dep]) -> String {
    let mut src = String::new();
    src.push_str("PROJECTS = ");
    src.push_str(&serde_starlark::to_string(deps).expect("failed to serialize"));
    sign_with_generated_header(Comment::Starlark, &src)
}

fn get_deps() -> Result<Vec<Dep>> {
    let out = Command::new("buck2")
        .arg("bxl")
        .arg("--reuse-current-config")
        .arg("antlir//antlir/distro/deps/projects.bxl:query")
        .output()
        .context("while running bxl")?;
    ensure!(
        out.status.success(),
        "bxl failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).context("while deserializing deps")
}

fn main() -> Result<()> {
    let out_path = output_path().context("while determining output path")?;
    // before trying to get the dependencies, replace the bzl file with some
    // known-good contents (empty) so that this binary can be ergonomically used
    // to resolve merge conflicts
    std::fs::write(&out_path, gen_bzl(&[])).context("while writing empty bzl")?;

    let deps = get_deps().context("while getting deps")?;
    let src = gen_bzl(&deps);
    std::fs::write(&out_path, src)
        .with_context(|| format!("while writing bzl at {}", out_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_in_sync() {
        let deps = get_deps().expect("failed to get deps");
        let src = gen_bzl(&deps);
        assert_eq!(src, include_str!("projects.bzl"),);
    }
}
