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
    /// `{name}_{version}_{arch}` identifiers of every package in the repo.
    /// The `repo()` rule downloads all of these (via the index) and
    /// exposes them as `packages` subtargets and in `YumRepoInfo`.
    /// Rendered manually (see `members_starlark`), so serde skips it.
    #[serde(default, skip_serializing)]
    pub members: BTreeMap<String, String>,
    /// Package names exposed as individually-buildable
    /// `package_aliases_subtargets` (resolved by name, first match wins).
    #[serde(
        default,
        skip_serializing_if = "BTreeSet::is_empty",
        alias = "package_subtargets"
    )]
    pub package_aliases_subtargets: BTreeSet<String>,
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

/// Architectures whose packages are identical modulo the arch suffix.
/// Members with one of these arches are arch-independent and always
/// belong in the plain list (as full NEVRAs).
const ARCH_INDEPENDENT: [&str; 3] = ["all", "any", "noarch"];

fn cpu_constraint(arch: &str) -> Option<Label> {
    match arch {
        "amd64" | "x86_64" => Label::new("ovr_config//cpu:x86_64").ok(),
        "arm64" | "aarch64" => Label::new("ovr_config//cpu:arm64").ok(),
        _ => None,
    }
}

/// Render the `members` attr value as `[...] + select({...})`:
/// arch-independent members (e.g. `_all`, `_noarch`) form the plain
/// list, and every binary-arch member goes into a `select()` branch
/// for its `ovr_config//cpu` constraint as a full NEVRA. All grouping
/// decisions are made here; the Starlark rules consume the resolved
/// list verbatim. Falls back to a plain list when a select is
/// impossible (single-arch repos, unknown arches).
fn members_starlark(
    members: &BTreeMap<String, String>,
) -> Result<Option<String>, buck_targets::Error> {
    if members.is_empty() {
        return Ok(None);
    }
    let render_list = |items: &BTreeSet<String>| -> Result<String, buck_targets::Error> {
        Ok(buck_targets::serde_starlark::to_string(&items)?
            .trim_end()
            .to_owned())
    };
    let binary_arches: BTreeSet<&str> = members
        .values()
        .map(|a| a.as_str())
        .filter(|a| !ARCH_INDEPENDENT.contains(a))
        .collect();
    if binary_arches.len() < 2 || binary_arches.iter().any(|a| cpu_constraint(a).is_none()) {
        return Ok(Some(render_list(&members.keys().cloned().collect())?));
    }

    let mut independent: BTreeSet<String> = BTreeSet::new();
    let mut by_arch: BTreeMap<Label, Vec<String>> = BTreeMap::new();
    for (nevra, arch) in members {
        if ARCH_INDEPENDENT.contains(&arch.as_str()) {
            independent.insert(nevra.clone());
        } else {
            by_arch
                .entry(cpu_constraint(arch).expect("checked above"))
                .or_default()
                .push(nevra.clone());
        }
    }

    let mut parts: Vec<String> = Vec::new();
    if !independent.is_empty() {
        parts.push(render_list(&independent)?);
    }
    if !by_arch.is_empty() {
        let select = Select::Other {
            by_constraint: by_arch,
            default: Some(Box::new(Vec::new())),
        };
        parts.push(
            buck_targets::serde_starlark::to_string(&select)?
                .trim_end()
                .to_owned(),
        );
    }
    Ok(Some(parts.join(" + ")))
}

/// Render a target struct with serde, then splice in the manually
/// rendered `members` attr (skipped by serde) when non-empty.
fn render_target<T: Serialize>(
    target: &T,
    members: &BTreeMap<String, String>,
) -> Result<String, buck_targets::Error> {
    let rendered = buck_targets::serde_starlark::to_string(target)?;
    let Some(expr) = members_starlark(members)? else {
        return Ok(rendered);
    };
    let body = rendered.trim_end();
    let Some(body) = body.strip_suffix(')') else {
        return Ok(rendered);
    };
    Ok(format!("{body}    members = {expr},\n)\n"))
}

impl buck_targets::ToStarlark for YumRepo {
    fn to_starlark(&self) -> Result<String, buck_targets::Error> {
        render_target(self, &self.members)
    }
}

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
    /// `{name}_{version}_{arch}` identifiers of every package in the suite.
    /// The `suite()` rule downloads all of these (via the index) and
    /// exposes them as `packages` subtargets and in `DebSuiteInfo`.
    /// Rendered manually (see `members_starlark`), so serde skips it.
    #[serde(default, skip_serializing)]
    pub members: BTreeMap<String, String>,
    #[serde(
        default,
        skip_serializing_if = "BTreeSet::is_empty",
        alias = "package_subtargets"
    )]
    pub package_aliases_subtargets: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_source: Option<Label>,
    pub snapshot_storage: BTreeMap<String, String>,
    pub visibility: Vec<String>,
}

impl buck_targets::ToStarlark for DebSuite {
    fn to_starlark(&self) -> Result<String, buck_targets::Error> {
        render_target(self, &self.members)
    }
}

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
            members: BTreeMap::new(),
            package_aliases_subtargets: BTreeSet::new(),
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
        // members and package_aliases_subtargets should be omitted when empty
        assert!(
            !starlark.contains("package_aliases_subtargets"),
            "empty subtargets should be omitted"
        );
        assert!(
            !starlark.contains("members"),
            "empty members should be omitted"
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
        let mut members = BTreeMap::new();
        members.insert(
            "basesystem_0:11-13.el9_noarch".to_owned(),
            "noarch".to_owned(),
        );
        let repo = YumRepo {
            name: "my-repo".to_owned(),
            arches: vec!["aarch64".to_owned(), "x86_64".to_owned()],
            baseurl: "https://example.com/{arch}/".to_owned(),
            members,
            package_aliases_subtargets: subtargets,
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
        assert!(
            starlark.contains("package_aliases_subtargets"),
            "subtargets: {starlark}"
        );
        assert!(starlark.contains("members = ["), "members: {starlark}");
        assert!(
            starlark.contains("basesystem_0:11-13.el9_noarch"),
            "member nevra: {starlark}"
        );
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
            members: BTreeMap::from([("bash_5.2-5_amd64".to_owned(), "amd64".to_owned())]),
            package_aliases_subtargets: BTreeSet::new(),
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
        assert!(
            starlark.contains("bash_5.2-5_amd64"),
            "members should be rendered: {starlark}"
        );
        assert!(
            !starlark.contains("package_aliases_subtargets"),
            "empty aliases should be omitted: {starlark}"
        );
    }

    #[test]
    fn members_dedupe_across_arches() {
        let cs = Checksums::new_sha256(hex!(
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        ));
        let suite = DebSuite {
            name: "trixie".to_owned(),
            architectures: vec!["amd64".to_owned(), "arm64".to_owned()],
            archive_url: "https://example.com/archive".to_owned(),
            components: vec!["main".to_owned()],
            distribution: "trixie".to_owned(),
            index_checksums: cs,
            members: BTreeMap::from([
                ("bash_5.2-5_amd64".to_owned(), "amd64".to_owned()),
                ("bash_5.2-5_arm64".to_owned(), "arm64".to_owned()),
                ("data_1.0_all".to_owned(), "all".to_owned()),
                ("onlyx_1.0_amd64".to_owned(), "amd64".to_owned()),
            ]),
            package_aliases_subtargets: BTreeSet::new(),
            snapshot_source: None,
            snapshot_storage: storage_map(),
            visibility: vec!["PUBLIC".to_owned()],
        };
        let starlark = suite.to_starlark().expect("renders");
        // Binary-arch members go into the select as full NEVRAs
        assert!(
            starlark.contains("select({"),
            "binary members should select: {starlark}"
        );
        assert!(
            starlark.contains("bash_5.2-5_amd64"),
            "amd64 nevra in select: {starlark}"
        );
        assert!(
            starlark.contains("bash_5.2-5_arm64"),
            "arm64 nevra in select: {starlark}"
        );
        assert!(
            starlark.contains("onlyx_1.0_amd64"),
            "single should be rendered: {starlark}"
        );
        // Arch-independent member stays in the plain members list
        assert!(
            starlark.contains("data_1.0_all"),
            "independent member: {starlark}"
        );
        assert!(
            !starlark.contains("members_pairs"),
            "no pairs attr: {starlark}"
        );
    }

    #[test]
    fn yum_members_dedupe_x86_64() {
        // Arch comes from Package::architecture, so x86_64 (which
        // contains an underscore) needs no string parsing at all.
        let members = BTreeMap::from([
            ("bash_5.2-5_x86_64".to_owned(), "x86_64".to_owned()),
            ("bash_5.2-5_aarch64".to_owned(), "aarch64".to_owned()),
            ("onlyarm_1.0-1_aarch64".to_owned(), "aarch64".to_owned()),
        ]);
        let expr = members_starlark(&members)
            .expect("renders")
            .expect("non-empty");
        assert!(expr.contains("select({"), "binary members select: {expr}");
        assert!(expr.contains("bash_5.2-5_x86_64"), "x86_64 nevra: {expr}");
        assert!(expr.contains("bash_5.2-5_aarch64"), "aarch64 nevra: {expr}");
        assert!(expr.contains("onlyarm_1.0-1_aarch64"), "single: {expr}");
    }

    #[test]
    fn package_aliases_subtargets_sorted() {
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
            members: BTreeMap::new(),
            package_aliases_subtargets: set,
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
