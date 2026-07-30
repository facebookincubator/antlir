/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! A typed wrapper around buck2's `select()` for the cases that show up across
//! antlir.

use std::collections::BTreeMap;

use buck_label::Label;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de::Error as _;
use thiserror::Error;

/// Error from [`Select::arch`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArchError {
    #[error("Select::arch requires at least one of x86_64, aarch64, noarch to be set")]
    Empty,
    #[error("Select::arch noarch is mutually exclusive with x86_64/aarch64")]
    NoarchConflict,
}

/// The `ovr_config//cpu` constraint for the x86_64 architecture.
const ARCH_X86_64: &str = "ovr_config//cpu:x86_64";
/// The `ovr_config//cpu` constraint for the aarch64 architecture. Note that
/// buck spells this `arm64`.
const ARCH_AARCH64: &str = "ovr_config//cpu:arm64";
/// Prefix for antlir2 OS constraints. The bare os name (e.g. `centos10`) is
/// appended to form the full constraint label.
const OS_PREFIX: &str = "antlir//antlir/antlir2/os:";
/// The catch-all branch key of a `select()`.
const DEFAULT: &str = "DEFAULT";

/// Package of the cpu constraints (the cell varies by alias, e.g. `ovr_config`
/// or `fbsource`, so classification matches on the package, not the full
/// label).
const CPU_PACKAGE: &str = "cpu";
/// The x86_64 cpu constraint name.
const CPU_X86_64: &str = "x86_64";
/// The aarch64 cpu constraint name (buck spells it `arm64`).
const CPU_ARM64: &str = "arm64";
/// Package of the antlir2 os constraints.
const OS_PACKAGE: &str = "antlir/antlir2/os";

/// A typed buck2 [`select()`](https://buck2.build/docs/rule_authors/configurations/).
///
/// Serializes to a starlark `select({ ... })` and deserializes from the JSON
/// emitted by `load_targets.bxl` (where selects are left unresolved). On
/// deserialization the concrete variant is reconstructed from the branch keys:
/// the two `ovr_config//cpu` arch constraints become [`Select::Arch`], keys
/// under `antlir//antlir/antlir2/os:` become [`Select::Os`], and anything else
/// becomes [`Select::Other`]. A map containing only `DEFAULT` becomes
/// [`Select::DefaultOnly`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Select<T> {
    /// A `select()` containing only its `DEFAULT` branch.
    DefaultOnly(T),
    /// `select()` on CPU architecture. At least one branch is set; a single
    /// arch still expands to a `select()` on that one arch. `noarch` expands to
    /// a `DEFAULT` branch. Build with [`Select::arch`] when `noarch` is mutually
    /// exclusive with per-arch branches, or [`Select::arch_with_noarch`] when
    /// they may be combined.
    Arch {
        /// Value for `ovr_config//cpu:x86_64`.
        x86_64: Option<T>,
        /// Value for `ovr_config//cpu:arm64`.
        aarch64: Option<T>,
        /// Architecture-independent value, emitted as the `DEFAULT` branch.
        noarch: Option<T>,
    },
    /// `select()` on antlir2 OS, keyed by the bare os name (e.g. `centos10`),
    /// which expands to `antlir//antlir/antlir2/os:{name}`.
    Os {
        /// Per-os values keyed by bare os name.
        by_os: BTreeMap<String, T>,
        /// Optional `DEFAULT` branch.
        default: Option<Box<T>>,
    },
    /// `select()` on arbitrary constraint [`Label`]s.
    Other {
        /// Per-constraint values keyed by the full constraint label.
        by_constraint: BTreeMap<Label, T>,
        /// Optional `DEFAULT` branch.
        default: Option<Box<T>>,
    },
}

#[bon::bon]
impl<T> Select<T> {
    /// `select()` containing only a `DEFAULT` branch.
    pub fn default_only(value: T) -> Self {
        Self::DefaultOnly(value)
    }

    /// `select()` over CPU architecture, via named parameters.
    ///
    /// All three branches are optional, but at least one must be set
    /// ([`ArchError::Empty`] otherwise). `noarch` is mutually exclusive with
    /// `x86_64`/`aarch64` ([`ArchError::NoarchConflict`] otherwise). A single
    /// per-arch branch still expands to a `select()` on that one arch; `noarch`
    /// expands to a `DEFAULT` branch.
    ///
    /// ```ignore
    /// Select::arch().x86_64(a).aarch64(b).call()?;
    /// Select::arch().x86_64(a).call()?;   // single-arch select
    /// Select::arch().noarch(a).call()?;   // DEFAULT-only select
    /// ```
    #[builder]
    pub fn arch(
        x86_64: Option<T>,
        aarch64: Option<T>,
        noarch: Option<T>,
    ) -> Result<Self, ArchError> {
        if x86_64.is_none() && aarch64.is_none() && noarch.is_none() {
            return Err(ArchError::Empty);
        }
        if noarch.is_some() && (x86_64.is_some() || aarch64.is_some()) {
            return Err(ArchError::NoarchConflict);
        }
        Ok(Self::Arch {
            x86_64,
            aarch64,
            noarch,
        })
    }

    /// `select()` over CPU architecture that permits a `DEFAULT` branch
    /// alongside architecture-specific branches.
    #[builder]
    pub fn arch_with_noarch(
        x86_64: Option<T>,
        aarch64: Option<T>,
        noarch: Option<T>,
    ) -> Result<Self, ArchError> {
        if x86_64.is_none() && aarch64.is_none() && noarch.is_none() {
            return Err(ArchError::Empty);
        }
        Ok(Self::Arch {
            x86_64,
            aarch64,
            noarch,
        })
    }

    /// `select()` over antlir2 OSes, keyed by bare os name, with no `DEFAULT`.
    pub fn os(by_os: BTreeMap<String, T>) -> Self {
        Self::Os {
            by_os,
            default: None,
        }
    }

    /// `select()` over antlir2 OSes, keyed by bare os name, with a `DEFAULT`.
    pub fn os_with_default(by_os: BTreeMap<String, T>, default: T) -> Self {
        Self::Os {
            by_os,
            default: Some(Box::new(default)),
        }
    }

    /// `select()` over arbitrary constraint labels, with no `DEFAULT`.
    pub fn other(by_constraint: BTreeMap<Label, T>) -> Self {
        Self::Other {
            by_constraint,
            default: None,
        }
    }

    /// `select()` over arbitrary constraint labels, with a `DEFAULT`.
    pub fn other_with_default(by_constraint: BTreeMap<Label, T>, default: T) -> Self {
        Self::Other {
            by_constraint,
            default: Some(Box::new(default)),
        }
    }

    /// The `DEFAULT` branch, if any (for [`Select::Arch`] this is `noarch`).
    pub fn default_branch(&self) -> Option<&T> {
        match self {
            Self::DefaultOnly(value) => Some(value),
            Self::Arch { noarch, .. } => noarch.as_ref(),
            Self::Os { default, .. } | Self::Other { default, .. } => default.as_deref(),
        }
    }
}

impl<T> Serialize for Select<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Collect the branches into a single ordered map of
        // condition-string -> value. A BTreeMap gives deterministic output and
        // sorts `DEFAULT` (uppercase) ahead of the lowercase constraint labels,
        // matching the usual buck convention.
        let mut entries: BTreeMap<String, &T> = BTreeMap::new();
        match self {
            Self::DefaultOnly(value) => {
                entries.insert(DEFAULT.to_owned(), value);
            }
            Self::Arch {
                x86_64,
                aarch64,
                noarch,
            } => {
                if let Some(noarch) = noarch {
                    entries.insert(DEFAULT.to_owned(), noarch);
                }
                if let Some(x86_64) = x86_64 {
                    entries.insert(ARCH_X86_64.to_owned(), x86_64);
                }
                if let Some(aarch64) = aarch64 {
                    entries.insert(ARCH_AARCH64.to_owned(), aarch64);
                }
            }
            Self::Os { by_os, default } => {
                for (name, value) in by_os {
                    entries.insert(format!("{OS_PREFIX}{name}"), value);
                }
                if let Some(default) = default {
                    entries.insert(DEFAULT.to_owned(), default);
                }
            }
            Self::Other {
                by_constraint,
                default,
            } => {
                for (label, value) in by_constraint {
                    entries.insert(label.to_string(), value);
                }
                if let Some(default) = default {
                    entries.insert(DEFAULT.to_owned(), default);
                }
            }
        }

        // serde_starlark renders a newtype struct as a positional function
        // call, so this produces `select({ ... })`.
        serializer.serialize_newtype_struct("select", &entries)
    }
}

impl<'de, T> Deserialize<'de> for Select<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // The bxl wraps an unresolved select in a single-key object keyed by
        // `__antlir_select__`; the value is the map of condition -> value.
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire<T> {
            #[serde(rename = "__antlir_select__")]
            entries: BTreeMap<String, T>,
        }

        let mut entries = Wire::<T>::deserialize(deserializer)?.entries;

        let default = entries.remove(DEFAULT).map(Box::new);

        if entries.is_empty()
            && let Some(default) = default
        {
            return Ok(Self::DefaultOnly(*default));
        }

        // Parse the remaining branch keys into labels so classification can be
        // cell-agnostic (buck canonicalizes cell aliases, e.g. it rewrites
        // `antlir//` to `fbcode//`, so matching on the package is robust).
        let labeled = entries
            .into_iter()
            .map(|(key, value)| Label::new(key).map(|label| (label, value)))
            .collect::<Result<Vec<(Label, T)>, _>>()
            .map_err(D::Error::custom)?;

        // Arch: every non-default branch is a cpu constraint. A DEFAULT-only
        // select was handled above as `DefaultOnly`.
        let is_arch = !labeled.is_empty()
            && labeled.iter().all(|(label, _)| {
                label.package() == CPU_PACKAGE
                    && (label.name() == CPU_X86_64 || label.name() == CPU_ARM64)
            });
        if is_arch {
            let mut x86_64 = None;
            let mut aarch64 = None;
            for (label, value) in labeled {
                if label.name() == CPU_X86_64 {
                    x86_64 = Some(value);
                } else {
                    aarch64 = Some(value);
                }
            }
            return Ok(Self::Arch {
                x86_64,
                aarch64,
                noarch: default.map(|value| *value),
            });
        }

        // Os: every branch key is an antlir2 os constraint.
        if !labeled.is_empty()
            && labeled
                .iter()
                .all(|(label, _)| label.package() == OS_PACKAGE)
        {
            let by_os = labeled
                .into_iter()
                .map(|(label, value)| (label.name().to_owned(), value))
                .collect();
            return Ok(Self::Os { by_os, default });
        }

        // Other: arbitrary constraint labels.
        Ok(Self::Other {
            by_constraint: labeled.into_iter().collect(),
            default,
        })
    }
}

#[cfg(test)]
mod tests {
    use maplit::btreemap;
    use serde_json::json;

    use super::*;

    /// Serialize a [`Select`] to its starlark form via serde_starlark.
    /// `serde_starlark::to_string` appends a trailing newline; trim it so the
    /// goldens below compare just the meaningful content.
    fn to_starlark<T: Serialize>(s: &Select<T>) -> String {
        serde_starlark::to_string(s)
            .expect("serialization is infallible for these types")
            .trim_end()
            .to_owned()
    }

    #[test]
    fn arch_serializes() {
        let s = Select::arch()
            .x86_64(vec!["x".to_owned()])
            .aarch64(vec!["a".to_owned()])
            .call()
            .expect("valid arch");
        // arm64 sorts before x86_64, so it is emitted first.
        assert_eq!(
            to_starlark(&s),
            r#"select({
    "ovr_config//cpu:arm64": ["a"],
    "ovr_config//cpu:x86_64": ["x"],
})"#,
        );
    }

    #[test]
    fn single_arch_serializes() {
        let s = Select::arch()
            .x86_64(vec!["x".to_owned()])
            .call()
            .expect("valid arch");
        assert_eq!(
            to_starlark(&s),
            r#"select({
    "ovr_config//cpu:x86_64": ["x"],
})"#,
        );
    }

    #[test]
    fn noarch_serializes() {
        let s = Select::arch()
            .noarch(vec!["n".to_owned()])
            .call()
            .expect("valid arch");
        assert_eq!(
            to_starlark(&s),
            r#"select({
    "DEFAULT": ["n"],
})"#,
        );
    }

    #[test]
    fn default_only_deserializes() {
        let s: Select<Vec<String>> =
            serde_json::from_value(wire(json!({"DEFAULT": ["n"]}))).expect("valid");
        assert_eq!(s, Select::default_only(vec!["n".to_owned()]));
        assert_eq!(
            to_starlark(&s),
            r#"select({
    "DEFAULT": ["n"],
})"#,
        );
    }

    #[test]
    fn arch_requires_a_branch() {
        // No branch set -> error. Annotate the value type since nothing infers it.
        let err = Select::<Vec<String>>::arch()
            .call()
            .expect_err("no branches");
        assert_eq!(err, ArchError::Empty);
    }

    #[test]
    fn noarch_excludes_per_arch() {
        let err = Select::arch()
            .x86_64(vec!["x".to_owned()])
            .noarch(vec!["n".to_owned()])
            .call()
            .expect_err("noarch + per-arch");
        assert_eq!(err, ArchError::NoarchConflict);
    }

    #[test]
    fn arch_with_noarch_serializes() {
        let s = Select::arch_with_noarch()
            .x86_64(vec!["x".to_owned()])
            .aarch64(vec!["a".to_owned()])
            .noarch(vec!["n".to_owned()])
            .call()
            .expect("valid mixed arch");
        assert_eq!(
            to_starlark(&s),
            r#"select({
    "DEFAULT": ["n"],
    "ovr_config//cpu:arm64": ["a"],
    "ovr_config//cpu:x86_64": ["x"],
})"#,
        );

        let roundtrip: Select<Vec<String>> = serde_json::from_value(wire(json!({
            "DEFAULT": ["n"],
            "ovr_config//cpu:arm64": ["a"],
            "ovr_config//cpu:x86_64": ["x"],
        })))
        .expect("mixed arch deserializes");
        assert_eq!(roundtrip, s);
    }

    #[test]
    fn os_serializes() {
        let s: Select<Vec<String>> = Select::os_with_default(
            btreemap! {
                "centos10".to_owned() => vec!["new".to_owned()],
            },
            vec!["old".to_owned()],
        );
        // DEFAULT (uppercase) sorts ahead of the lowercase constraint label.
        assert_eq!(
            to_starlark(&s),
            r#"select({
    "DEFAULT": ["old"],
    "antlir//antlir/antlir2/os:centos10": ["new"],
})"#,
        );
    }

    #[test]
    fn other_serializes() {
        let s: Select<Vec<String>> = Select::other(btreemap! {
            Label::new("ovr_config//os:linux").expect("valid") => vec!["l".to_owned()],
        });
        assert_eq!(
            to_starlark(&s),
            r#"select({
    "ovr_config//os:linux": ["l"],
})"#,
        );
    }

    /// Build the bxl-style wire JSON for a select from its entries.
    fn wire(entries: serde_json::Value) -> serde_json::Value {
        json!({ "__antlir_select__": entries })
    }

    #[test]
    fn arch_deserializes() {
        let v = wire(json!({
            "ovr_config//cpu:x86_64": ["x"],
            "ovr_config//cpu:arm64": ["a"],
        }));
        let s: Select<Vec<String>> = serde_json::from_value(v).expect("valid");
        assert_eq!(
            s,
            Select::arch()
                .x86_64(vec!["x".to_owned()])
                .aarch64(vec!["a".to_owned()])
                .call()
                .expect("valid arch"),
        );
    }

    #[test]
    fn single_arch_deserializes() {
        let v = wire(json!({
            "ovr_config//cpu:x86_64": ["x"],
        }));
        let s: Select<Vec<String>> = serde_json::from_value(v).expect("valid");
        assert_eq!(
            s,
            Select::arch()
                .x86_64(vec!["x".to_owned()])
                .call()
                .expect("valid arch"),
        );
    }

    #[test]
    fn os_deserializes() {
        let v = wire(json!({
            "antlir//antlir/antlir2/os:centos10": ["new"],
            "DEFAULT": ["old"],
        }));
        let s: Select<Vec<String>> = serde_json::from_value(v).expect("valid");
        assert_eq!(
            s,
            Select::os_with_default(
                btreemap! { "centos10".to_owned() => vec!["new".to_owned()] },
                vec!["old".to_owned()],
            )
        );
    }

    #[test]
    fn other_deserializes() {
        let v = wire(json!({
            "ovr_config//os:linux": ["l"],
        }));
        let s: Select<Vec<String>> = serde_json::from_value(v).expect("valid");
        assert_eq!(
            s,
            Select::other(btreemap! {
                Label::new("ovr_config//os:linux").expect("valid") => vec!["l".to_owned()],
            })
        );
    }

    /// buck canonicalizes the `antlir//` cell alias to `fbcode//` in the labels
    /// it emits, so os classification must still recognize that form and
    /// re-serialize it back to the canonical `antlir//` spelling.
    #[test]
    fn canonicalized_os_deserializes() {
        let v = wire(json!({
            "fbcode//antlir/antlir2/os:centos10": ["new"],
            "DEFAULT": ["old"],
        }));
        let s: Select<Vec<String>> = serde_json::from_value(v).expect("valid");
        assert_eq!(
            s,
            Select::os_with_default(
                btreemap! { "centos10".to_owned() => vec!["new".to_owned()] },
                vec!["old".to_owned()],
            )
        );
        // Re-serializes to the `antlir//` spelling regardless of input cell.
        assert!(
            to_starlark(&s).contains("\"antlir//antlir/antlir2/os:centos10\""),
            "got {}",
            to_starlark(&s)
        );
    }

    #[test]
    fn arch_with_default_deserializes() {
        let v = wire(json!({
            "ovr_config//cpu:x86_64": ["x"],
            "ovr_config//cpu:arm64": ["a"],
            "DEFAULT": ["d"],
        }));
        let s: Select<Vec<String>> = serde_json::from_value(v).expect("valid");
        assert_eq!(
            s,
            Select::arch_with_noarch()
                .x86_64(vec!["x".to_owned()])
                .aarch64(vec!["a".to_owned()])
                .noarch(vec!["d".to_owned()])
                .call()
                .expect("valid mixed arch"),
        );
    }

    /// Mixed os + non-os keys -> Other, not Os.
    #[test]
    fn mixed_keys_is_other() {
        let v = wire(json!({
            "antlir//antlir/antlir2/os:centos10": ["c"],
            "ovr_config//os:linux": ["l"],
        }));
        let s: Select<Vec<String>> = serde_json::from_value(v).expect("valid");
        assert!(matches!(s, Select::Other { .. }), "got {s:?}");
    }

    /// Every variant round-trips: deserialize from wire JSON, re-serialize to
    /// starlark, and the text is stable across a second round-trip.
    #[rstest::rstest]
    #[case::arch(json!({
        "ovr_config//cpu:x86_64": ["x"],
        "ovr_config//cpu:arm64": ["a"],
    }))]
    #[case::os(json!({
        "DEFAULT": ["old"],
        "antlir//antlir/antlir2/os:centos10": ["new"],
    }))]
    #[case::other(json!({
        "ovr_config//os:linux": ["l"],
        "ovr_config//os:macos": ["m"],
    }))]
    #[case::default_only(json!({
        "DEFAULT": ["d"],
    }))]
    fn round_trips(#[case] entries: serde_json::Value) {
        let first: Select<Vec<String>> = serde_json::from_value(wire(entries)).expect("valid");
        let starlark = to_starlark(&first);
        // Deserializing the same wire JSON again yields an equal value, so
        // re-serialization is stable.
        let second = first.clone();
        assert_eq!(starlark, to_starlark(&second));
    }
}
