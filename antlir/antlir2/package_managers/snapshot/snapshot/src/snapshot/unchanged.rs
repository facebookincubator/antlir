/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/// Dedicated process exit code for "snapshot skipped, nothing changed".
/// The skycastle job (`tools/skycastle/workflows2/antlir/snapshot.sky`)
/// classifies the snapshot action as `Skipped` when it sees this code
/// together with [`SKIP_MARKER`], which disables the remaining
/// verify/commit/submit steps instead of submitting a useless diff.
pub(crate) const EXIT_CODE_SKIPPED_NO_CHANGES: u8 = 42;

/// Log marker emitted (at a level visible in default job logs) when a
/// snapshot is skipped because every target's metadata is unchanged.
pub(crate) const SKIP_MARKER: &str = "SNAPSHOT_SKIP_NO_CHANGES";

/// Signal that every target's metadata is unchanged, so the whole
/// snapshot is skipped. `main` turns this into
/// [`EXIT_CODE_SKIPPED_NO_CHANGES`].
#[derive(Debug, thiserror::Error)]
#[error("{SKIP_MARKER}: metadata unchanged, skipping snapshot")]
pub(crate) struct SkippedNoChanges;
