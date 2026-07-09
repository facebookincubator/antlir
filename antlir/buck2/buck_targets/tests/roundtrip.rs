/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! End-to-end round-trip test: read the `tests/fixture` package back with the
//! companion `load_targets.bxl` (via a real `buck2 bxl` invocation), deserialize
//! into the strict [`MyRule`] type through typetag, and confirm the rendered
//! `BuckFile` is identical to one built from hand-constructed equivalents. This
//! exercises the bxl wire format, typetag tag dispatch and [`Select`]
//! reconstruction against real buck output.

use std::path::PathBuf;
use std::process::Command;

use buck_label::Label;
use buck_label::Package;
use buck_targets::BuckFile;
use buck_targets::BuckTarget;
use buck_targets::Load;
use buck_targets::Select;
use buck_targets::ToStarlark;
use buck_targets::impl_to_starlark_via_serde;
use maplit::btreemap;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
#[serde(rename = "my_rule")]
struct MyRule {
    name: String,
    srcs: Vec<String>,
    deps: Select<Vec<String>>,
}

impl_to_starlark_via_serde!(MyRule);

#[typetag::serde(name = "my_rule")]
impl BuckTarget for MyRule {
    fn name(&self) -> &str {
        &self.name
    }

    fn loads(&self) -> Vec<Load> {
        vec![
            Load::builder()
                .bzl(
                    Label::new("fbcode//antlir/buck2/buck_targets/tests/fixture:defs.bzl")
                        .expect("valid label"),
                )
                .symbol("my_rule")
                .build(),
        ]
    }
}

/// Walk up from the current directory to the repo root (the dir with a
/// `.buckconfig`) so `buck2` runs against the real checkout.
fn repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().expect("have a current directory");
    loop {
        if dir.join(".buckconfig").exists() {
            return dir;
        }
        if !dir.pop() {
            panic!("could not find a .buckconfig walking up from the cwd");
        }
    }
}

fn fixture_package() -> Package {
    Package::new("fbcode//antlir/buck2/buck_targets/tests/fixture").expect("valid package")
}

fn buck_file(targets: Vec<Box<dyn BuckTarget>>) -> String {
    BuckFile::builder()
        .oncall("antlir")
        .package(fixture_package())
        .generated_by("roundtrip-test")
        .targets(targets)
        .build()
        .to_starlark()
        .expect("renders")
}

#[test]
fn fixture_round_trips() {
    let output = Command::new("buck2")
        .current_dir(repo_root())
        .arg("--isolation-dir=buck_targets_roundtrip_test")
        .arg("bxl")
        .arg("fbcode//antlir/buck2/buck_targets/load_targets.bxl:main")
        .arg("--")
        .arg("--package=fbcode//antlir/buck2/buck_targets/tests/fixture:")
        .output()
        .expect("can spawn buck2");
    assert!(
        output.status.success(),
        "bxl failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let from_bxl: Vec<Box<dyn BuckTarget>> = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("deserializing bxl output failed: {e}"));

    // The fixture has three targets; `gamma` carries the macro-expanded label
    // and must be skipped, leaving only the two top-level targets.
    let mut names: Vec<&str> = from_bxl.iter().map(|t| t.name()).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["alpha", "beta"],
        "macro-expanded target should be skipped by the dumper"
    );

    let rendered_from_bxl = buck_file(from_bxl);

    // The same targets, built directly. The fixture mirrors these exactly.
    let expected: Vec<Box<dyn BuckTarget>> = vec![
        Box::new(MyRule {
            name: "alpha".to_owned(),
            srcs: vec!["alpha.rs".to_owned()],
            deps: Select::arch()
                .x86_64(vec!["//x:x".to_owned()])
                .aarch64(vec!["//a:a".to_owned()])
                .call()
                .expect("valid arch"),
        }),
        Box::new(MyRule {
            name: "beta".to_owned(),
            srcs: vec!["beta.rs".to_owned()],
            deps: Select::os_with_default(
                btreemap! { "centos10".to_owned() => vec!["//c:c".to_owned()] },
                vec!["//d:d".to_owned()],
            ),
        }),
    ];
    let rendered_expected = buck_file(expected);

    assert_eq!(
        rendered_from_bxl, rendered_expected,
        "round-tripped buck file should match the hand-built one"
    );
}
