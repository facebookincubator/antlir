/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! [`BuckFile`]: a whole `BUCK`/`TARGETS` file made of [`BuckTarget`]s.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;

use buck_label::Label;
use buck_label::Package;
use thiserror::Error;

use crate::BuckTarget;
use crate::ToStarlark;

#[derive(Debug, Error)]
pub enum Error {
    #[error("duplicate target name in package: '{name}'")]
    DuplicateTarget { name: String },
    #[error("failed to serialize to starlark")]
    Serialize(#[from] serde_starlark::Error),
    #[error("failed to write '{path}'")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

mod sealed {
    pub trait Sealed {}
}

/// The element type a [`BuckFile`] can hold: either a concrete `T: BuckTarget`
/// (a homogeneous file) or a `Box<dyn BuckTarget>` (a file mixing rule types).
/// Both yield a `&dyn BuckTarget` for rendering. Sealed: not implementable
/// outside this crate.
pub trait AsDynBuckTarget: sealed::Sealed {
    #[doc(hidden)]
    fn as_dyn(&self) -> &dyn BuckTarget;
}

impl<T: BuckTarget> sealed::Sealed for T {}
impl sealed::Sealed for Box<dyn BuckTarget> {}

impl<T: BuckTarget> AsDynBuckTarget for T {
    fn as_dyn(&self) -> &dyn BuckTarget {
        self
    }
}

impl AsDynBuckTarget for Box<dyn BuckTarget> {
    fn as_dyn(&self) -> &dyn BuckTarget {
        &**self
    }
}

/// A complete `BUCK`/`TARGETS` file: an `oncall`, the [`Package`] it lives in,
/// a note about what generated it, and the list of targets.
///
/// The target type `T` defaults to `Box<dyn BuckTarget>` for files that mix rule
/// types, but a file whose targets are all the same rule can use the concrete
/// type directly (e.g. `BuckFile<RustLibrary>` over a `Vec<RustLibrary>`) for an
/// allocation-free, downcast-free API.
///
/// [`BuckFile::to_string`] renders the `# @generated` header (carrying
/// `generated_by`), the de-duplicated `load()` block, the `oncall(...)` line and
/// the targets sorted by name (erroring on duplicate names).
///
/// By default the rendered file is signed with a `SignedSource` token. Some
/// generated files are intentionally left unsigned (e.g. a central file that
/// every contributor appends to, where a signature would be a permanent
/// merge-conflict hotspot); set `signed(false)` for those. An optional
/// `warning` block of comment lines can be inserted right after the
/// `# @generated` header (e.g. to tell humans not to hand-edit the file).
#[derive(bon::Builder)]
pub struct BuckFile<T = Box<dyn BuckTarget>> {
    #[builder(into)]
    oncall: String,
    package: Package,
    #[builder(into)]
    generated_by: String,
    targets: Vec<T>,
    /// Whether to render a `SignedSource` token (and sign the file). Defaults to
    /// `true`. When `false` the `# @generated` header carries no token and the
    /// file is left unsigned.
    #[builder(default = true)]
    signed: bool,
    /// Optional comment block inserted after the `# @generated` header. Each
    /// line is rendered as `# <line>`; supply the text without the leading
    /// `# `.
    #[builder(into)]
    warning: Option<String>,
}

impl<T> BuckFile<T> {
    /// The `oncall` for this file.
    pub fn oncall(&self) -> &str {
        &self.oncall
    }

    /// The [`Package`] this file lives in.
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// A short description of what generated this file (goes in the
    /// `# @generated` header).
    pub fn generated_by(&self) -> &str {
        &self.generated_by
    }

    /// Whether this file is rendered with a `SignedSource` token.
    pub fn signed(&self) -> bool {
        self.signed
    }

    /// The optional comment block rendered after the `# @generated` header.
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    /// The targets in this file, in declaration order (rendering sorts them by
    /// name).
    pub fn targets(&self) -> &[T] {
        &self.targets
    }

    /// Append a single target to the file.
    pub fn push(&mut self, target: T) {
        self.targets.push(target);
    }

    /// Append all targets from an iterator to the file.
    pub fn extend(&mut self, targets: impl IntoIterator<Item = T>) {
        self.targets.extend(targets);
    }
}

impl<T> ToStarlark for BuckFile<T>
where
    T: AsDynBuckTarget,
{
    /// Render the file to a signed, `# @generated` starlark string.
    fn to_starlark(&self) -> Result<String, Error> {
        // Sort targets by name and reject duplicates.
        let mut targets: Vec<&dyn BuckTarget> =
            self.targets.iter().map(AsDynBuckTarget::as_dyn).collect();
        targets.sort_by(|a, b| a.name().cmp(b.name()));
        for pair in targets.windows(2) {
            if pair[0].name() == pair[1].name() {
                return Err(Error::DuplicateTarget {
                    name: pair[0].name().to_owned(),
                });
            }
        }

        // Collect and de-duplicate loads, grouped by `.bzl` file.
        let mut loads: BTreeMap<Label, BTreeSet<String>> = BTreeMap::new();
        for target in &targets {
            for load in target.loads() {
                loads
                    .entry(load.bzl().clone())
                    .or_default()
                    .insert(load.symbol().to_owned());
            }
        }

        let mut out = String::new();
        if self.signed {
            writeln!(
                out,
                "# \x40generated by {} {}",
                self.generated_by,
                signedsource::TOKEN
            )
            .expect("writing to a String is infallible");
        } else {
            writeln!(out, "# \x40generated by {}", self.generated_by)
                .expect("writing to a String is infallible");
        }
        if let Some(warning) = &self.warning {
            for line in warning.lines() {
                writeln!(out, "# {line}").expect("writing to a String is infallible");
            }
        }
        out.push('\n');

        if !loads.is_empty() {
            for (bzl, symbols) in &loads {
                let symbols = symbols
                    .iter()
                    .map(|s| format!("\"{s}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                // buck2 `load()` requires the explicit-cell form `@cell//...`
                // (a bare `cell//...` is rejected); `Label` renders without the
                // leading `@`, so add it here.
                writeln!(out, "load(\"@{bzl}\", {symbols})")
                    .expect("writing to a String is infallible");
            }
            out.push('\n');
        }

        writeln!(out, "oncall(\"{}\")", self.oncall).expect("writing to a String is infallible");

        for target in &targets {
            out.push('\n');
            // `to_starlark()` (via serde_starlark) appends a trailing newline;
            // trim it so targets are separated by exactly one blank line and the
            // file ends with a single newline rather than a trailing blank line.
            out.push_str(target.to_starlark()?.trim_end());
            out.push('\n');
        }

        if self.signed {
            Ok(signedsource::sign(&out).expect("the SignedSource token is definitely present"))
        } else {
            Ok(out)
        }
    }
}

impl<T> BuckFile<T>
where
    T: AsDynBuckTarget,
{
    /// Render the file and write it to `path`.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        let contents = self.to_starlark()?;
        std::fs::write(path, contents).map_err(|source| Error::Write {
            path: path.to_owned(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use buck_label::Label;
    use serde::Deserialize;
    use serde::Serialize;
    use serde_json::json;

    use super::*;
    use crate::Load;
    use crate::Select;
    use crate::impl_to_starlark_via_serde;

    #[derive(Serialize, Deserialize)]
    #[serde(rename = "test_rule")]
    struct TestRule {
        name: String,
        srcs: Vec<String>,
        deps: Select<Vec<String>>,
    }

    impl_to_starlark_via_serde!(TestRule);

    #[typetag::serde(name = "test_rule")]
    impl BuckTarget for TestRule {
        fn name(&self) -> &str {
            &self.name
        }

        fn loads(&self) -> Vec<Load> {
            vec![
                Load::builder()
                    .bzl(Label::new("antlir//antlir/bzl:build_defs.bzl").expect("valid"))
                    .symbol("test_rule")
                    .build(),
            ]
        }
    }

    fn test_target(name: &str) -> Box<dyn BuckTarget> {
        Box::new(TestRule {
            name: name.to_owned(),
            srcs: vec![format!("{name}.rs")],
            deps: Select::arch()
                .x86_64(vec!["x".to_owned()])
                .aarch64(vec!["a".to_owned()])
                .call()
                .expect("valid arch"),
        })
    }

    fn render(targets: Vec<Box<dyn BuckTarget>>) -> Result<String, Error> {
        BuckFile::builder()
            .oncall("antlir")
            .package(Package::new("antlir//antlir/buck2/buck_targets").expect("valid"))
            .generated_by("my-tool")
            .targets(targets)
            .build()
            .to_starlark()
    }

    #[test]
    fn full_file_renders() {
        // Pass the targets out of name order to exercise sorting.
        let out = render(vec![test_target("beta"), test_target("alpha")]).expect("renders");

        // Header carries generated_by and a SignedSource token.
        let header = out.lines().next().expect("has a first line");
        assert!(
            header.starts_with("# \x40generated by my-tool SignedSource<<"),
            "unexpected header: {header}"
        );

        // The load block is present and de-duplicated to a single line. Loads
        // are rendered in the explicit-cell form `@cell//...` that buck2
        // `load()` requires.
        assert!(
            out.contains("\nload(\"@antlir//antlir/bzl:build_defs.bzl\", \"test_rule\")\n"),
            "missing load block in:\n{out}"
        );
        assert_eq!(
            out.matches("load(").count(),
            1,
            "load should be de-duplicated across both targets"
        );

        assert!(out.contains("\noncall(\"antlir\")\n"), "missing oncall in:\n{out}");

        // Both rules are present, alpha before beta.
        let alpha = out.find("name = \"alpha\"").expect("alpha present");
        let beta = out.find("name = \"beta\"").expect("beta present");
        assert!(alpha < beta, "targets should be sorted by name");

        // The select is emitted unresolved.
        assert!(
            out.contains("deps = select({"),
            "missing select in:\n{out}"
        );
    }

    #[test]
    fn duplicate_targets_error() {
        let err = render(vec![test_target("dup"), test_target("dup")]).expect_err("should error");
        assert!(
            matches!(&err, Error::DuplicateTarget { name } if name == "dup"),
            "unexpected error: {err:?}"
        );
    }

    /// A typed target serialized to starlark equals the same target after a
    /// JSON round-trip through typetag (the read-back path used by the bxl).
    #[test]
    fn typetag_round_trip() {
        let original = test_target("widget");
        let starlark = original.to_starlark().expect("renders");

        // The externally-tagged JSON that `load_targets.bxl` would emit for
        // this target (rule name as the tag, select left unresolved).
        let wire = json!({
            "test_rule": {
                "name": "widget",
                "srcs": ["widget.rs"],
                "deps": {
                    "__antlir_select__": {
                        "ovr_config//cpu:x86_64": ["x"],
                        "ovr_config//cpu:arm64": ["a"],
                    }
                }
            }
        });

        let parsed: Box<dyn BuckTarget> =
            serde_json::from_value(wire).expect("deserializes to the concrete type via typetag");
        let round_tripped = parsed.to_starlark().expect("renders");

        assert_eq!(starlark, round_tripped);
        assert_eq!(parsed.name(), "widget");
    }

    fn concrete_test_rule(name: &str) -> TestRule {
        TestRule {
            name: name.to_owned(),
            srcs: vec![format!("{name}.rs")],
            deps: Select::arch()
                .x86_64(vec!["x".to_owned()])
                .aarch64(vec!["a".to_owned()])
                .call()
                .expect("valid arch"),
        }
    }

    /// A homogeneous `BuckFile<TestRule>` (concrete element type, no boxing)
    /// renders identically to the `Box<dyn BuckTarget>` form.
    #[test]
    fn homogeneous_file_matches_boxed() {
        let boxed = render(vec![test_target("alpha"), test_target("beta")]).expect("renders");

        let concrete = BuckFile::builder()
            .oncall("antlir")
            .package(Package::new("antlir//antlir/buck2/buck_targets").expect("valid"))
            .generated_by("my-tool")
            .targets(vec![concrete_test_rule("alpha"), concrete_test_rule("beta")])
            .build()
            .to_starlark()
            .expect("renders");

        assert_eq!(concrete, boxed);
    }

    #[test]
    fn unsigned_with_warning() {
        let out = BuckFile::builder()
            .oncall("antlir")
            .package(Package::new("antlir//antlir/buck2/buck_targets").expect("valid"))
            .generated_by("my-tool")
            .targets(vec![test_target("alpha")])
            .signed(false)
            .warning("Do NOT edit this file manually.\nIt is regenerated by automation.")
            .build()
            .to_starlark()
            .expect("renders");

        // Header carries no SignedSource token when unsigned.
        let header = out.lines().next().expect("has a first line");
        assert_eq!(header, "# \x40generated by my-tool", "unexpected header: {header}");
        assert!(
            !out.contains("SignedSource"),
            "unsigned file should carry no SignedSource token:\n{out}"
        );

        // The warning lines are rendered as `# <line>` right after the header.
        assert!(
            out.contains("\n# Do NOT edit this file manually.\n# It is regenerated by automation.\n\n"),
            "missing warning block in:\n{out}"
        );

        // The body still renders normally.
        assert!(out.contains("\noncall(\"antlir\")\n"), "missing oncall in:\n{out}");
        assert!(out.contains("name = \"alpha\""), "missing target in:\n{out}");
    }

    #[test]
    fn push_and_extend() {
        let mut file = BuckFile::builder()
            .oncall("antlir")
            .package(Package::new("antlir//antlir/buck2/buck_targets").expect("valid"))
            .generated_by("my-tool")
            .targets(Vec::<Box<dyn BuckTarget>>::new())
            .build();

        file.push(test_target("alpha"));
        file.extend(vec![test_target("beta"), test_target("gamma")]);
        assert_eq!(file.targets().len(), 3);

        let out = file.to_starlark().expect("renders");
        assert!(out.contains("name = \"alpha\""), "{out}");
        assert!(out.contains("name = \"gamma\""), "{out}");
    }
}
