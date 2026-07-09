/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! [`ToStarlark`]: render a value to starlark source.

use crate::Error;

/// Render a value to its starlark source form.
///
/// Implemented for [`BuckFile`](crate::BuckFile) (renders the whole file) and
/// required by [`BuckTarget`](crate::BuckTarget) (renders one `rule_name(...)`
/// call). Rule structs almost always want the trivial
/// `serde_starlark::to_string` implementation — use [`impl_to_starlark_via_serde!`] rather
/// than writing it by hand.
pub trait ToStarlark {
    /// Render `self` to starlark source.
    fn to_starlark(&self) -> Result<String, Error>;
}

/// Implement [`ToStarlark`] for one or more types by proxying to
/// `serde_starlark::to_string`.
///
/// Each type must implement `serde::Serialize` (rule structs typically
/// `#[derive(Serialize)]` with `#[serde(rename = "rule_name")]` so the output is
/// `rule_name(...)`).
///
/// ```ignore
/// #[derive(serde::Serialize, serde::Deserialize)]
/// #[serde(rename = "rust_library")]
/// struct RustLibrary { /* ... */ }
///
/// buck_targets::impl_to_starlark_via_serde!(RustLibrary);
/// ```
#[macro_export]
macro_rules! impl_to_starlark_via_serde {
    ($($t:ty),+ $(,)?) => {
        $(
            impl $crate::ToStarlark for $t {
                fn to_starlark(
                    &self,
                ) -> ::core::result::Result<::std::string::String, $crate::Error> {
                    ::core::result::Result::Ok($crate::serde_starlark::to_string(self)?)
                }
            }
        )+
    };
}
