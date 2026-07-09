/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! The [`BuckTarget`] trait that rule structs implement, plus the [`Load`]
//! statement helper.

use buck_label::Label;

use crate::ToStarlark;

/// A single symbol imported via a starlark `load()` statement, e.g.
/// `load("antlir//antlir/bzl:build_defs.bzl", "rust_library")`.
///
/// `bzl` is a [`Label`] pointing at the `.bzl` file (its `name()` is the
/// filename, e.g. `build_defs.bzl`); `symbol` is the imported identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, bon::Builder)]
pub struct Load {
    bzl: Label,
    #[builder(into)]
    symbol: String,
}

impl Load {
    /// The [`Label`] of the `.bzl` file this symbol is loaded from.
    pub fn bzl(&self) -> &Label {
        &self.bzl
    }

    /// The imported symbol name.
    pub fn symbol(&self) -> &str {
        &self.symbol
    }
}

/// A buck target (rule invocation) that can be written into a `BUCK`/`TARGETS`
/// file.
///
/// Implementors are normal strict structs that `#[derive(Serialize,
/// Deserialize)]` and carry `#[serde(rename = "rule_name")]` so that
/// serde_starlark emits `rule_name(...)`. The `#[typetag::serde(name =
/// "rule_name")]` attribute on the `impl` registers the same rule name as the
/// tag used to reconstruct the concrete type when reading a file back.
///
/// Every implementor must also implement [`ToStarlark`]; use
/// [`impl_to_starlark_via_serde!`](crate::impl_to_starlark_via_serde) for the trivial
/// `serde_starlark::to_string` proxy.
///
/// ```ignore
/// #[derive(serde::Serialize, serde::Deserialize, bon::Builder)]
/// #[serde(rename = "rust_library")]
/// struct RustLibrary { name: String, /* ... */ }
///
/// buck_targets::impl_to_starlark_via_serde!(RustLibrary);
///
/// #[typetag::serde(name = "rust_library")]
/// impl BuckTarget for RustLibrary {
///     fn name(&self) -> &str { &self.name }
///     fn loads(&self) -> Vec<Load> { /* ... */ }
/// }
/// ```
#[typetag::serde]
pub trait BuckTarget: ToStarlark + 'static {
    /// The target name, unique within its package.
    fn name(&self) -> &str;

    /// The `load()`s this rule's starlark serialization requires.
    fn loads(&self) -> Vec<Load>;
}
