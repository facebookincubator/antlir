/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

use buck_label::Label;
use buck_targets::BuckTarget;
use buck_targets::Load;
use buck_targets::Select;
use serde::Deserialize;
use serde::Serialize;
use snapshot_common::Checksums;

/// `repomd_checksums` can be either a single dict (when all arches share the
/// same checksums) or a `select()` keyed by `ovr_config//cpu` constraints.
/// `Checksums` now serializes as a dict natively via custom `Serialize` in
/// `snapshot_common`, so no `checksums_to_dict` helper is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RepomdChecksums {
    Single(Checksums),
    Multi(Select<Checksums>),
}

/// A `repo()` target from `//antlir/antlir2/package_managers/yum:repo.bzl`
/// rendered via `buck_targets`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "repo")]
pub struct YumRepo {
    pub name: String,
    pub arches: Vec<String>,
    pub baseurl: String,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub package_subtargets: BTreeSet<String>,
    #[serde(
        rename = "index_checksums",
        alias = "repomd_checksums",
        alias = "snapshot_index_checksums"
    )]
    pub index_checksums: RepomdChecksums,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_source: Option<Label>,
    pub snapshot_storage: BTreeMap<String, String>,
    pub visibility: Vec<String>,
}

buck_targets::impl_to_starlark_via_serde!(YumRepo);

#[typetag::serde(name = "repo")]
impl BuckTarget for YumRepo {
    fn name(&self) -> &str {
        &self.name
    }

    fn loads(&self) -> Vec<Load> {
        vec![
            Load::builder()
                .bzl(
                    Label::new("antlir//antlir/antlir2/package_managers/yum:repo.bzl")
                        .expect("valid label"),
                )
                .symbol("repo")
                .build(),
        ]
    }
}

/// A `suite()` target from `//antlir/antlir2/package_managers/deb:suite.bzl`
/// rendered via `buck_targets`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "suite")]
pub struct DebSuite {
    pub name: String,
    pub architectures: Vec<String>,
    pub archive_url: String,
    pub components: Vec<String>,
    pub distribution: String,
    #[serde(
        rename = "index_checksums",
        alias = "inrelease_checksums",
        alias = "snapshot_index_checksums"
    )]
    pub index_checksums: Checksums,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub package_subtargets: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_source: Option<Label>,
    pub snapshot_storage: BTreeMap<String, String>,
    pub visibility: Vec<String>,
}

buck_targets::impl_to_starlark_via_serde!(DebSuite);

#[typetag::serde(name = "suite")]
impl BuckTarget for DebSuite {
    fn name(&self) -> &str {
        &self.name
    }

    fn loads(&self) -> Vec<Load> {
        vec![
            Load::builder()
                .bzl(
                    Label::new("antlir//antlir/antlir2/package_managers/deb:suite.bzl")
                        .expect("valid label"),
                )
                .symbol("suite")
                .build(),
        ]
    }
}

/// A single buck target paired with the file it should be written to. This
/// replaces the old `BuckTargetDescriptor` which carried a raw starlark
/// snippet.
pub struct DescribedTarget {
    pub package_path: PathBuf,
    pub target: Box<dyn BuckTarget>,
}

/// Helper to build `RepomdChecksums` from a list of `(arch, arch_modifier,
/// checksums)` tuples. Always builds a `select()` even when checksums are
/// identical across arches – duplication is fine and simplifies logic.
pub fn make_repomd_checksums(
    entries: &[(&str, &str, &Checksums)],
) -> anyhow::Result<RepomdChecksums> {
    if entries.is_empty() {
        anyhow::bail!("no architectures found");
    }

    // Detect duplicate arch entries early (last-wins previously hid bugs)
    let mut seen_arches = BTreeSet::new();
    for (arch, _, _) in entries {
        if !seen_arches.insert(*arch) {
            anyhow::bail!("duplicate arch entry '{}'", arch);
        }
    }

    let mut x86_64: Option<Checksums> = None;
    let mut aarch64: Option<Checksums> = None;
    let mut other: BTreeMap<Label, Checksums> = BTreeMap::new();

    for (arch, modifier, cs) in entries {
        match *arch {
            "x86_64" => x86_64 = Some((*cs).clone()),
            "aarch64" => aarch64 = Some((*cs).clone()),
            _ => {
                let label = Label::new((*modifier).to_owned())
                    .map_err(|e| anyhow::anyhow!("invalid modifier {}: {}", modifier, e))?;
                if other.insert(label.clone(), (*cs).clone()).is_some() {
                    anyhow::bail!("duplicate modifier '{}'", modifier);
                }
            }
        }
    }

    if !other.is_empty() {
        // When custom arches exist, include x86_64/aarch64 if present, but avoid overwriting
        // an explicit custom entry that already uses the same label.
        if let Some(cs) = x86_64 {
            let lbl = Label::new("ovr_config//cpu:x86_64").expect("valid");
            other.entry(lbl).or_insert(cs);
        }
        if let Some(cs) = aarch64 {
            let lbl = Label::new("ovr_config//cpu:arm64").expect("valid");
            other.entry(lbl).or_insert(cs);
        }
        Ok(RepomdChecksums::Multi(Select::other(other)))
    } else {
        let sel = match (x86_64, aarch64) {
            (Some(x), Some(a)) => Select::arch().x86_64(x).aarch64(a).call(),
            (Some(x), None) => Select::arch().x86_64(x).call(),
            (None, Some(a)) => Select::arch().aarch64(a).call(),
            (None, None) => anyhow::bail!("no architectures for arch select"),
        }
        .map_err(|e| anyhow::anyhow!("invalid arch select: {}", e))?;
        Ok(RepomdChecksums::Multi(sel))
    }
}

#[cfg(test)]
mod tests {
    use buck_targets::ToStarlark as _;
    use hex_literal::hex;

    use super::*;

    fn storage_map() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("type".to_owned(), "manifold".to_owned()),
            ("bucket".to_owned(), "test".to_owned()),
            ("api_key".to_owned(), "key".to_owned()),
        ])
    }

    #[test]
    fn yum_repo_renders_with_single_checksums() {
        let cs = Checksums::new_sha256(hex!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        let repo = YumRepo {
            name: "my-repo".to_owned(),
            arches: vec!["x86_64".to_owned(), "aarch64".to_owned()],
            baseurl: "https://example.com/{arch}/".to_owned(),
            package_subtargets: BTreeSet::new(),
            index_checksums: RepomdChecksums::Single(cs),
            snapshot_source: None,
            snapshot_storage: storage_map(),
            visibility: vec!["PUBLIC".to_owned()],
        };
        let starlark = repo.to_starlark().expect("renders");
        assert!(starlark.contains("repo("), "should be repo rule");
        assert!(starlark.contains("name = \"my-repo\""), "name");
        assert!(starlark.contains("arches = ["), "arches");
        assert!(starlark.contains("baseurl ="), "baseurl");
        assert!(
            starlark.contains("index_checksums ="),
            "checksums should be generic index_checksums: {starlark}"
        );
        assert!(
            starlark.contains(&"a".repeat(64)),
            "checksum value should be hex encoded"
        );
        // Must be dict, not Checksums() constructor – custom Serialize in
        // snapshot_common::Checksums makes it a dict.
        assert!(
            !starlark.contains("Checksums("),
            "should serialize as dict, not Checksums(): {starlark}"
        );
        assert!(starlark.contains("snapshot_storage ="), "storage");
        // package_subtargets should be omitted when empty
        assert!(
            !starlark.contains("package_subtargets"),
            "empty subtargets should be omitted"
        );
    }

    #[test]
    fn yum_repo_renders_with_select_checksums() {
        let c1 = Checksums::new_sha256(hex!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        let c2 = Checksums::new_sha256(hex!(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        ));
        let repomd = make_repomd_checksums(&[
            ("aarch64", "ovr_config//cpu:arm64", &c1),
            ("x86_64", "ovr_config//cpu:x86_64", &c2),
        ])
        .expect("make");

        let mut subtargets = BTreeSet::new();
        subtargets.insert("basesystem".to_owned());
        let repo = YumRepo {
            name: "my-repo".to_owned(),
            arches: vec!["aarch64".to_owned(), "x86_64".to_owned()],
            baseurl: "https://example.com/{arch}/".to_owned(),
            package_subtargets: subtargets,
            index_checksums: repomd,
            snapshot_source: None,
            snapshot_storage: storage_map(),
            visibility: vec!["PUBLIC".to_owned()],
        };
        let starlark = repo.to_starlark().expect("renders");
        assert!(starlark.contains("select("), "should be select");
        assert!(starlark.contains("ovr_config//cpu:arm64"), "arm64 key");
        assert!(starlark.contains("ovr_config//cpu:x86_64"), "x86 key");
        assert!(starlark.contains(&"a".repeat(64)), "aaa hex");
        assert!(starlark.contains(&"b".repeat(64)), "bbb hex");
        assert!(starlark.contains("package_subtargets"), "subtargets");
        assert!(
            starlark.contains("index_checksums ="),
            "should be generic index_checksums: {starlark}"
        );
        assert!(
            !starlark.contains("Checksums("),
            "should not contain Checksums() constructor: {starlark}"
        );
    }

    #[test]
    fn deb_suite_renders() {
        let cs = Checksums::new_sha256(hex!(
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        ));
        let suite = DebSuite {
            name: "trixie".to_owned(),
            architectures: vec!["amd64".to_owned()],
            archive_url: "https://example.com/archive".to_owned(),
            components: vec!["main".to_owned()],
            distribution: "trixie".to_owned(),
            index_checksums: cs,
            package_subtargets: BTreeSet::new(),
            snapshot_source: None,
            snapshot_storage: storage_map(),
            visibility: vec!["PUBLIC".to_owned()],
        };
        let starlark = suite.to_starlark().expect("renders");
        assert!(starlark.contains("suite("), "suite rule");
        assert!(starlark.contains("archive_url ="), "archive_url");
        assert!(
            starlark.contains("deadbeef"),
            "checksum should contain the hex pattern"
        );
        assert!(
            starlark.contains("index_checksums ="),
            "should be generic index_checksums: {starlark}"
        );
        assert!(
            !starlark.contains("Checksums("),
            "should serialize checksums as dict, not Checksums(): {starlark}"
        );
    }

    #[test]
    fn package_subtargets_sorted() {
        // BTreeSet should sort
        let mut set = BTreeSet::new();
        set.insert("zebra".to_owned());
        set.insert("alpha".to_owned());
        set.insert("middle".to_owned());
        let cs = Checksums::new_sha256(hex!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        let repo = YumRepo {
            name: "test".to_owned(),
            arches: vec!["x86_64".to_owned()],
            baseurl: "https://example.com/{arch}/".to_owned(),
            package_subtargets: set,
            index_checksums: RepomdChecksums::Single(cs),
            snapshot_source: None,
            snapshot_storage: storage_map(),
            visibility: vec!["PUBLIC".to_owned()],
        };
        let starlark = repo.to_starlark().expect("renders");
        let alpha_pos = starlark.find("alpha").expect("alpha");
        let middle_pos = starlark.find("middle").expect("middle");
        let zebra_pos = starlark.find("zebra").expect("zebra");
        assert!(alpha_pos < middle_pos && middle_pos < zebra_pos, "sorted");
    }

    #[test]
    fn repomd_does_not_collapse_when_same() {
        // Same checksums across arches should still produce a select() – the
        // duplication is acceptable and simplifies the code.
        let c = Checksums::new_sha256(hex!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        let repomd = make_repomd_checksums(&[
            ("aarch64", "ovr_config//cpu:arm64", &c),
            ("x86_64", "ovr_config//cpu:x86_64", &c),
        ])
        .expect("make");
        match repomd {
            RepomdChecksums::Single(_) => panic!("should not collapse to single"),
            RepomdChecksums::Multi(select) => {
                // Should contain both arches – use serde_starlark directly
                // since Select implements Serialize as select({...}) but not
                // ToStarlark.
                let rendered = buck_targets::serde_starlark::to_string(&select).expect("renders");
                assert!(rendered.contains("arm64"));
                assert!(rendered.contains("x86_64"));
            }
        }
    }

    #[test]
    fn duplicate_arch_bails() {
        let c = Checksums::new_sha256(hex!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        let err = make_repomd_checksums(&[
            ("x86_64", "ovr_config//cpu:x86_64", &c),
            ("x86_64", "ovr_config//cpu:x86_64", &c),
        ])
        .expect_err("duplicate arch should bail");
        assert!(err.to_string().contains("duplicate"));
    }
}
