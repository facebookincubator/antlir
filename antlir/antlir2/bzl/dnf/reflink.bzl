# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

REFLINK_FLAVORS = {
    # @oss-disable[end= ]: "centos10": "antlir//antlir/antlir2/facebook/images/build_appliance/centos10:rpm2extents",
    # @oss-disable[end= ]: "centos9": "antlir//antlir/antlir2/facebook/images/build_appliance/centos9:rpm2extents",
}

def rpm2extents(
        *,
        ctx: AnalysisContext,
        appliance: Dependency,
        rpm: Artifact,
        extents: Artifact,
        identifier: str | None = None):
    ctx.actions.run(
        cmd_args(
            appliance[RunInfo],
            rpm,
            extents.as_output(),
        ),
        env = {"RUST_LOG": "trace"},
        category = "rpm2extents",
        identifier = identifier,
    )
