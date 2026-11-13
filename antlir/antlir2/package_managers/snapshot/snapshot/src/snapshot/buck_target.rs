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
    /// Rendered manually (see `render_target`: `members_compressed`
    /// when `compress_members` applies, else `members_starlark`), so
    /// serde skips it.
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

/// Canonical arch spellings for one package manager, in mask-bit order
/// (see `compress_members`): arch-independent, x86, arm. The Starlark
/// rule impls expand compressed members with the same spellings.
struct MemberArchNames {
    independent: &'static str,
    x86: &'static str,
    arm: &'static str,
}

const DEB_ARCH_NAMES: MemberArchNames = MemberArchNames {
    independent: "all",
    x86: "amd64",
    arm: "arm64",
};

const YUM_ARCH_NAMES: MemberArchNames = MemberArchNames {
    independent: "noarch",
    x86: "x86_64",
    arm: "aarch64",
};

/// Minimum number of stems that must share a `-`-terminated name
/// prefix for `compress_members` to factor it out as a group. Smaller
/// families stay in the ungrouped (`""`) bucket as full stems.
const MEMBERS_MIN_GROUP: usize = 4;

fn cpu_constraint(arch: &str) -> Option<Label> {
    match arch {
        "amd64" | "x86_64" => Label::new("ovr_config//cpu:x86_64").ok(),
        "arm64" | "aarch64" => Label::new("ovr_config//cpu:arm64").ok(),
        _ => None,
    }
}

/// Group member stems by shared name prefix, or `None` when the
/// members cannot be represented this way (the caller then falls back
/// to the plain `members` list).
///
/// A *stem* is a NEVRA without its `_{arch}` suffix. The result maps a
/// shared prefix to the stem remainders under it, one entry per stem
/// as `{rest}|{arches}`, where `arches` names the arches the stem
/// exists for using the letters `a` (arch-independent), `x` (x86) and
/// `r` (arm) in that order -- e.g. `us_1.0-1|a` or `dev_2.0-1|xr`. A
/// stem present for both binary arches is stored once.
///
/// Prefixes are discovered, not hardcoded: every `-`-terminated
/// prefix of a package name (the part of the stem before the first
/// `_`) shared by at least `MEMBERS_MIN_GROUP` stems is a candidate,
/// and each stem joins the longest candidate it matches (so
/// `asterisk-core-sounds-en-` wins over `asterisk-`). Groups that
/// still end up with fewer than `MEMBERS_MIN_GROUP` entries (their
/// stems mostly chose longer prefixes) are dissolved back into the
/// `""` group, whose stems stay whole. Keeping
/// one line per package inside its family group keeps the generated
/// file glanceable and makes member changes single-line diffs next
/// to their relatives. The Starlark decoder is `expand_members` in
/// `//antlir/antlir2/package_managers/snapshot:members.bzl`; keep the
/// two in sync.
///
/// Compression applies whenever the members are representable (any
/// combination of the canonical arches, single-arch included);
/// `render_target` only picks it when it renders smaller than the
/// plain list, so tiny member sets keep the plain form. Members that
/// cannot be represented (non-canonical arch spelling, separator in
/// a stem) keep the plain rendering.
fn compress_members(
    members: &BTreeMap<String, String>,
    arch_names: &MemberArchNames,
) -> Option<BTreeMap<String, Vec<String>>> {
    let mut stem_masks: BTreeMap<String, u8> = BTreeMap::new();
    for (nevra, arch) in members {
        let bit = if arch == arch_names.independent {
            1
        } else if arch == arch_names.x86 {
            2
        } else if arch == arch_names.arm {
            4
        } else {
            return None;
        };
        let stem = nevra.strip_suffix(&format!("_{arch}"))?;
        if !stem.is_ascii() || stem.contains('|') {
            return None;
        }
        *stem_masks.entry(stem.to_owned()).or_default() |= bit;
    }

    // Count how many stems share each `-`-terminated name prefix.
    let mut prefix_counts: BTreeMap<String, usize> = BTreeMap::new();
    for stem in stem_masks.keys() {
        for prefix in name_prefixes(stem) {
            *prefix_counts.entry(prefix.to_owned()).or_default() += 1;
        }
    }

    // Assign each stem to its longest qualifying prefix, then
    // dissolve groups that end up below the threshold anyway.
    let mut assigned: Vec<(&str, &str, u8)> = Vec::new();
    let mut group_sizes: BTreeMap<&str, usize> = BTreeMap::new();
    for (stem, mask) in &stem_masks {
        let prefix = name_prefixes(stem)
            .into_iter()
            .filter(|p| prefix_counts.get(*p).copied().unwrap_or(0) >= MEMBERS_MIN_GROUP)
            .max_by_key(|p| p.len())
            .unwrap_or("");
        *group_sizes.entry(prefix).or_default() += 1;
        assigned.push((prefix, stem, *mask));
    }

    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (prefix, stem, mask) in assigned {
        let prefix = if prefix.is_empty() || group_sizes[prefix] >= MEMBERS_MIN_GROUP {
            prefix
        } else {
            ""
        };
        let arches: String = [(1, 'a'), (2, 'x'), (4, 'r')]
            .into_iter()
            .filter(|(bit, _)| mask & bit != 0)
            .map(|(_, c)| c)
            .collect();
        groups
            .entry(prefix.to_owned())
            .or_default()
            .push(format!("{}|{arches}", &stem[prefix.len()..]));
    }
    Some(groups)
}

/// Every `-`-terminated prefix of the package name in a stem (the part
/// before the first `_`), shortest first: `libfoo-bar_1.0` yields
/// `libfoo-`.
fn name_prefixes(stem: &str) -> Vec<&str> {
    let name = stem.split('_').next().unwrap_or("");
    name.match_indices('-').map(|(i, _)| &name[..=i]).collect()
}

/// Render the `members_compressed` attr value (a prefix-grouped dict
/// from [`compress_members`]), or `None` to fall back to the plain
/// `members` list.
fn members_compressed_starlark(
    members: &BTreeMap<String, String>,
    arch_names: &MemberArchNames,
) -> Result<Option<String>, buck_targets::Error> {
    let Some(groups) = compress_members(members, arch_names) else {
        return Ok(None);
    };
    Ok(Some(
        buck_targets::serde_starlark::to_string(&groups)?
            .trim_end()
            .to_owned(),
    ))
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

/// Indent every line of a rendered `members` expression after the
/// first by one level. `serde_starlark` renders the list/select at
/// indent 0, but it is spliced in as an attribute value (indent 1), so
/// its entries belong at indent 2, its closing brackets at indent 1.
fn indent_members_expr(expr: &str) -> String {
    let mut lines = expr.lines();
    let mut indented = lines.next().unwrap_or_default().to_owned();
    for line in lines {
        indented.push('\n');
        if !line.is_empty() {
            indented.push_str("    ");
            indented.push_str(line);
        }
    }
    indented
}

/// Render a target struct with serde, then splice in the manually
/// rendered members attr (skipped by serde) when non-empty: the
/// compressed form when [`compress_members`] renders smaller,
/// otherwise the plain `members` list (see `members_starlark`).
fn render_target<T: Serialize>(
    target: &T,
    members: &BTreeMap<String, String>,
    arch_names: &MemberArchNames,
) -> Result<String, buck_targets::Error> {
    let rendered = buck_targets::serde_starlark::to_string(target)?;
    if members.is_empty() {
        return Ok(rendered);
    }
    let plain = members_starlark(members)?;
    let compressed = members_compressed_starlark(members, arch_names)?;
    let (attr, expr) = match (plain, compressed) {
        (Some(p), Some(c)) if c.len() < p.len() => ("members_compressed", c),
        (Some(p), _) => ("members", p),
        (None, Some(c)) => ("members_compressed", c),
        (None, None) => return Ok(rendered),
    };
    let body = rendered.trim_end();
    let Some(body) = body.strip_suffix(')') else {
        return Ok(rendered);
    };
    Ok(format!(
        "{body}    {attr} = {},\n)\n",
        indent_members_expr(&expr)
    ))
}

impl buck_targets::ToStarlark for YumRepo {
    fn to_starlark(&self) -> Result<String, buck_targets::Error> {
        render_target(self, &self.members, &YUM_ARCH_NAMES)
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
    /// Rendered manually (see `render_target`: `members_compressed`
    /// when `compress_members` applies, else `members_starlark`), so
    /// serde skips it.
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
        render_target(self, &self.members, &DEB_ARCH_NAMES)
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

    /// Reference decoder mirroring `expand_members` in
    /// `//antlir/antlir2/package_managers/snapshot:members.bzl`:
    /// reconstruct the NEVRAs visible for `current_arch` (the empty
    /// string models a platform matching neither binary arch).
    fn expand_for_test(
        groups: &BTreeMap<String, Vec<String>>,
        arch_names: &MemberArchNames,
        current_arch: &str,
    ) -> BTreeSet<String> {
        let spellings = [
            (arch_names.independent, 'a'),
            (arch_names.x86, 'x'),
            (arch_names.arm, 'r'),
        ];
        let mut out = BTreeSet::new();
        for (prefix, entries) in groups {
            for entry in entries {
                let (rest, arches) = entry.split_once('|').expect("entry has arch letters");
                let stem = format!("{prefix}{rest}");
                for (spelling, letter) in spellings {
                    if arches.contains(letter) && (letter == 'a' || spelling == current_arch) {
                        out.insert(format!("{stem}_{spelling}"));
                    }
                }
            }
        }
        out
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
        // Both binary arches present: members render compressed, with
        // each stem stored once, and expand back to the same NEVRAs
        // per arch as the plain rendering would select.
        assert!(
            starlark.contains("members_compressed = {"),
            "members should be compressed: {starlark}"
        );
        assert!(
            !starlark.contains("bash_5.2-5_amd64"),
            "full nevras should not appear: {starlark}"
        );
        let groups = compress_members(&suite.members, &DEB_ARCH_NAMES).expect("compressible");
        for (current, arches) in [
            ("amd64", &["all", "amd64"][..]),
            ("arm64", &["all", "arm64"][..]),
            ("", &["all"][..]),
        ] {
            let expected: BTreeSet<String> = suite
                .members
                .iter()
                .filter(|(_, a)| arches.contains(&a.as_str()))
                .map(|(n, _)| n.clone())
                .collect();
            assert_eq!(
                expand_for_test(&groups, &DEB_ARCH_NAMES, current),
                expected,
                "expansion for arch '{current}'"
            );
        }
    }

    #[test]
    fn members_are_indented_as_an_attribute_value() {
        let cs = Checksums::new_sha256(hex!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        // Enough members that serde_starlark renders the grouped dict
        // multi-line rather than inlining it.
        let mut members = BTreeMap::from([
            ("bash_5.2-5_x86_64".to_owned(), "x86_64".to_owned()),
            ("bash_5.2-5_aarch64".to_owned(), "aarch64".to_owned()),
        ]);
        for i in 0..200 {
            members.insert(format!("data-{i}_1.0_noarch"), "noarch".to_owned());
        }
        let repo = YumRepo {
            name: "my-repo".to_owned(),
            arches: vec!["aarch64".to_owned(), "x86_64".to_owned()],
            baseurl: "https://example.com/{arch}/".to_owned(),
            members,
            package_aliases_subtargets: BTreeSet::new(),
            index_checksums: RepomdChecksums::Single(cs),
            snapshot_source: None,
            snapshot_storage: storage_map(),
            visibility: vec!["PUBLIC".to_owned()],
        };
        let starlark = repo.to_starlark().expect("renders");
        // Compressed groups sit at indent 2 and the dict closes at
        // indent 1, like any other attribute value. The `data-`
        // family (200 stems) is factored out as its own group.
        assert!(
            starlark.contains("    members_compressed = {\n        \""),
            "compressed members indentation: {starlark}"
        );
        assert!(
            starlark.contains("\"data-\": ["),
            "family group should be factored out: {starlark}"
        );
        assert!(
            starlark.ends_with("    },\n)\n"),
            "compressed dict and target closing indentation: {starlark}"
        );
        let groups = compress_members(&repo.members, &YUM_ARCH_NAMES).expect("compressible");
        assert_eq!(
            expand_for_test(&groups, &YUM_ARCH_NAMES, "x86_64"),
            repo.members
                .iter()
                .filter(|(_, a)| a.as_str() != "aarch64")
                .map(|(n, _)| n.clone())
                .collect::<BTreeSet<_>>(),
            "x86_64 expansion"
        );
    }

    #[test]
    fn yum_members_compress_x86_64() {
        // Arch comes from Package::architecture, so x86_64 (which
        // contains an underscore) needs no string parsing at all.
        let members = BTreeMap::from([
            ("bash_5.2-5_x86_64".to_owned(), "x86_64".to_owned()),
            ("bash_5.2-5_aarch64".to_owned(), "aarch64".to_owned()),
            ("onlyarm_1.0-1_aarch64".to_owned(), "aarch64".to_owned()),
        ]);
        let groups = compress_members(&members, &YUM_ARCH_NAMES).expect("compressible");
        // The stem shared by both binary arches is stored once.
        assert_eq!(
            groups.values().map(Vec::len).sum::<usize>(),
            2,
            "stems stored once: {groups:?}"
        );
        assert_eq!(
            expand_for_test(&groups, &YUM_ARCH_NAMES, "x86_64"),
            BTreeSet::from(["bash_5.2-5_x86_64".to_owned()]),
            "x86_64 expansion"
        );
        assert_eq!(
            expand_for_test(&groups, &YUM_ARCH_NAMES, "aarch64"),
            BTreeSet::from([
                "bash_5.2-5_aarch64".to_owned(),
                "onlyarm_1.0-1_aarch64".to_owned(),
            ]),
            "aarch64 expansion"
        );
    }

    #[test]
    fn compressed_members_round_trip_across_groups() {
        // A large family is factored into its own prefix group; the
        // longest shared prefix wins over shorter ones.
        let mut members = BTreeMap::new();
        for i in 0..200 {
            for arch in ["amd64", "arm64"] {
                members.insert(
                    format!("asterisk-core-sounds-en-{:03}_1.0-1_{arch}", i),
                    arch.to_owned(),
                );
            }
        }
        members.insert("zz-data_1.0-1_all".to_owned(), "all".to_owned());
        let groups = compress_members(&members, &DEB_ARCH_NAMES).expect("compressible");
        let family = groups
            .get("asterisk-core-sounds-en-")
            .expect("family group should exist");
        assert_eq!(family.len(), 200, "family entries: {family:?}");
        assert!(
            family.iter().all(|e| e.ends_with("|xr")),
            "both-arch entries carry xr: {family:?}"
        );
        assert!(
            groups
                .get("")
                .expect("ungrouped bucket")
                .contains(&"zz-data_1.0-1|a".to_owned()),
            "ungrouped stem stays whole: {groups:?}"
        );
        for current in ["amd64", "arm64"] {
            let expected: BTreeSet<String> = members
                .iter()
                .filter(|(_, a)| a.as_str() == "all" || a.as_str() == current)
                .map(|(n, _)| n.clone())
                .collect();
            assert_eq!(
                expand_for_test(&groups, &DEB_ARCH_NAMES, current),
                expected,
                "expansion for {current}"
            );
        }
        // Compressed rendering must actually shrink the plain one.
        let plain = members_starlark(&members)
            .expect("renders")
            .expect("non-empty");
        let compressed = members_compressed_starlark(&members, &DEB_ARCH_NAMES)
            .expect("renders")
            .expect("compressed");
        assert!(
            compressed.len() < plain.len(),
            "compressed {} should beat plain {}",
            compressed.len(),
            plain.len()
        );
    }

    #[test]
    fn compress_members_dissolves_small_groups() {
        // `arctica-` is shared by 5 stems, but 4 of them prefer the
        // longer `arctica-session-` prefix, leaving `arctica-` with a
        // single entry. Factoring out a prefix for one stem saves
        // nothing, so the group is dissolved into the `""` bucket.
        let mut members = BTreeMap::new();
        members.insert("arctica-greeter_1.0_amd64".to_owned(), "amd64".to_owned());
        for name in ["alpha", "beta", "gamma", "delta"] {
            members.insert(
                format!("arctica-session-{name}_1.0_amd64"),
                "amd64".to_owned(),
            );
        }
        let groups = compress_members(&members, &DEB_ARCH_NAMES).expect("compressible");
        assert!(
            !groups.contains_key("arctica-"),
            "single-entry group should be dissolved: {groups:?}"
        );
        assert_eq!(
            groups.get("arctica-session-").map(Vec::len),
            Some(4),
            "full group survives: {groups:?}"
        );
        assert!(
            groups
                .get("")
                .expect("ungrouped bucket")
                .contains(&"arctica-greeter_1.0|x".to_owned()),
            "dissolved stem stays whole: {groups:?}"
        );
        assert_eq!(
            expand_for_test(&groups, &DEB_ARCH_NAMES, "amd64"),
            members.keys().cloned().collect::<BTreeSet<_>>(),
            "expansion still complete"
        );
    }

    #[test]
    fn compress_members_falls_back() {
        // Single binary arch compresses like any other representable
        // set (this is what lets single-arch suites such as noble
        // shrink); `render_target` decides plain-vs-compressed by
        // rendered size, not here.
        let single = BTreeMap::from([
            ("bash_5.2-5_amd64".to_owned(), "amd64".to_owned()),
            ("data_1.0_all".to_owned(), "all".to_owned()),
        ]);
        let groups = compress_members(&single, &DEB_ARCH_NAMES).expect("compressible");
        assert_eq!(
            expand_for_test(&groups, &DEB_ARCH_NAMES, "amd64"),
            single.keys().cloned().collect::<BTreeSet<_>>(),
            "single-arch expansion"
        );
        assert_eq!(
            expand_for_test(&groups, &DEB_ARCH_NAMES, "arm64"),
            BTreeSet::from(["data_1.0_all".to_owned()]),
            "other arch sees only arch-independent members"
        );
        // Non-canonical arch spelling (`any` is arch-independent but not
        // the canonical `all` a deb decoder would emit).
        let any_arch = BTreeMap::from([
            ("bash_5.2-5_amd64".to_owned(), "amd64".to_owned()),
            ("bash_5.2-5_arm64".to_owned(), "arm64".to_owned()),
            ("data_1.0_any".to_owned(), "any".to_owned()),
        ]);
        assert_eq!(compress_members(&any_arch, &DEB_ARCH_NAMES), None);
        // Separator character in a stem.
        let separator = BTreeMap::from([
            ("ba|sh_5.2-5_amd64".to_owned(), "amd64".to_owned()),
            ("ba|sh_5.2-5_arm64".to_owned(), "arm64".to_owned()),
        ]);
        assert_eq!(compress_members(&separator, &DEB_ARCH_NAMES), None);
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
