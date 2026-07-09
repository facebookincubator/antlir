/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Generic generation (and read-back) of buck `BUCK`/`TARGETS` files.
//!
//! This crate consolidates the recipe that is reimplemented across antlir for
//! programmatically emitting buck files: a [`BuckFile`] owns the `# @generated`
//! header, the `load(...)` statements, the `oncall(...)` line and the sorted,
//! de-duplicated list of [`BuckTarget`]s. Rule authors implement the
//! [`BuckTarget`] trait on their own strict types; [`Select`] models a buck2
//! `select()` for the common arch/os/arbitrary cases.
//!
//! Files written by [`BuckFile`] can be read back into the *same* strict rule
//! types via `typetag` (keyed on the buck rule name) by feeding the JSON dumped
//! by the companion `load_targets.bxl` into
//! [`serde_json`], so a generated file round-trips.

mod file;
mod select;
mod target;
mod to_starlark;

// Re-exported so `impl_to_starlark_via_serde!` works without callers depending on
// serde_starlark directly.
#[doc(hidden)]
pub use serde_starlark;

pub use file::AsDynBuckTarget;
pub use file::BuckFile;
pub use file::Error;
pub use select::ArchError;
pub use select::Select;
pub use target::BuckTarget;
pub use target::Load;
pub use to_starlark::ToStarlark;
