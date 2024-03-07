# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")

REFLINK_FLAVORS = {
    # @oss-disable
    # @oss-disable
}

def rpm2extents(
        ctx: AnalysisContext,
        rpm2extents_in_ba: RunInfo,
        rpm: Artifact,
        extents: Artifact,
        build_appliance: Dependency,
        identifier: str | None = None):
    ctx.actions.run(
        cmd_args(
            rpm2extents_in_ba,
            ensure_single_output(build_appliance),
            rpm,
            extents.as_output(),
        ),
        env = {"RUST_LOG": "trace"},
        category = "rpm2extents",
        identifier = identifier,
    )
