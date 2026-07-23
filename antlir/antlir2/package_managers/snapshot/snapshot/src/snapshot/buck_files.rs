/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use buck_label::Package;
use buck_targets::BuckFile;
use buck_targets::BuckTarget;
use buck_targets::ToStarlark as _;

use super::buck_target::DescribedTarget;

/// Group described targets by their `package_path` and render each group as a
/// BUCK file using `buck_targets::BuckFile`, which handles `# @generated`
/// header, `load()` deduplication, `oncall()`, sorting by name and
/// SignedSource signing.
pub fn render_all(targets: Vec<DescribedTarget>) -> Result<BTreeMap<PathBuf, String>> {
    let mut by_package: BTreeMap<PathBuf, Vec<Box<dyn BuckTarget>>> = BTreeMap::new();
    for dt in targets {
        by_package.entry(dt.package_path).or_default().push(dt.target);
    }

    by_package
        .into_iter()
        .map(|(path, group)| {
            let package = package_from_path(&path)
                .with_context(|| format!("invalid package path {}", path.display()))?;
            let content = render_buck_file(&package, group)
                .with_context(|| format!("failed to render {}", path.display()))?;
            Ok((path, content))
        })
        .collect()
}

fn package_from_path(file_path: &Path) -> Result<Package> {
    let parent = file_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", file_path.display()))?;
    if parent.as_os_str().is_empty() {
        bail!("empty package path for {}", file_path.display());
    }
    // Buck package format: <cell>//<path>, where cell is first component, rest is remaining path.
    let mut components = parent.components();
    let cell_comp = components
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty parent for {}", file_path.display()))?;
    let cell_str = match cell_comp {
        Component::Normal(os) => os
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-utf8 cell in {}", file_path.display()))?,
        _ => bail!("invalid cell component in {}", file_path.display()),
    };

    let rest_parts: Vec<String> = components
        .map(|c| match c {
            Component::Normal(os) => os
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("non-utf8 in {}", file_path.display()))
                .map(|s| s.to_owned()),
            _ => bail!("invalid component in {}", file_path.display()),
        })
        .collect::<Result<Vec<_>>>()?;

    let package_str = if rest_parts.is_empty() {
        format!("{cell_str}//")
    } else {
        format!("{cell_str}//{}", rest_parts.join("/"))
    };

    Package::new(package_str).context("invalid package")
}

fn render_buck_file(package: &Package, targets: Vec<Box<dyn BuckTarget>>) -> Result<String> {
    BuckFile::builder()
        .oncall("antlir")
        .package(package.clone())
        .generated_by("snapshot")
        .targets(targets)
        .build()
        .to_starlark()
        .map_err(|e| anyhow::anyhow!("failed to render BUCK file for {package}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use buck_label::Label;
    use buck_targets::BuckTarget;
    use buck_targets::Load;
    use serde::Deserialize;
    use serde::Serialize;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename = "test_rule")]
    struct TestRule {
        name: String,
        value: String,
    }

    buck_targets::impl_to_starlark_via_serde!(TestRule);

    #[typetag::serde(name = "test_rule")]
    impl BuckTarget for TestRule {
        fn name(&self) -> &str {
            &self.name
        }

        fn loads(&self) -> Vec<Load> {
            vec![
                Load::builder()
                    .bzl(
                        Label::new("antlir//antlir/bzl:build_defs.bzl")
                            .expect("valid label"),
                    )
                    .symbol("test_rule")
                    .build(),
            ]
        }
    }

    #[test]
    fn test_render_all_merges_loads() {
        let targets = vec![
            DescribedTarget {
                package_path: PathBuf::from("fbcode/test/BUCK"),
                target: Box::new(TestRule {
                    name: "a".to_owned(),
                    value: "1".to_owned(),
                }),
            },
            DescribedTarget {
                package_path: PathBuf::from("fbcode/test/BUCK"),
                target: Box::new(TestRule {
                    name: "b".to_owned(),
                    value: "2".to_owned(),
                }),
            },
        ];

        let rendered = render_all(targets).expect("render should succeed");
        let content = rendered
            .get(&PathBuf::from("fbcode/test/BUCK"))
            .expect("exists");
        // BuckFile deduplicates loads.
        assert_eq!(content.matches("load(").count(), 1);
        assert!(content.contains("name = \"a\""), "first target present");
        assert!(content.contains("name = \"b\""), "second target present");
    }

    #[test]
    fn test_multiple_packages() {
        let targets = vec![
            DescribedTarget {
                package_path: PathBuf::from("fbcode/test1/BUCK"),
                target: Box::new(TestRule {
                    name: "a".to_owned(),
                    value: "1".to_owned(),
                }),
            },
            DescribedTarget {
                package_path: PathBuf::from("fbcode/test2/BUCK"),
                target: Box::new(TestRule {
                    name: "b".to_owned(),
                    value: "2".to_owned(),
                }),
            },
        ];
        let rendered = render_all(targets).expect("render");
        assert_eq!(rendered.len(), 2);
    }
}
