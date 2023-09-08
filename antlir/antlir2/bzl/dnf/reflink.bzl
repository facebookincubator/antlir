# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "//antlir/antlir2/bzl:types.bzl",
    "LayerInfo",  # @unused Used as type
)

REFLINK_FLAVORS = {
    "centos8": "//antlir/antlir2/facebook/images/build_appliance/centos8:build-appliance",
    "centos9": "//antlir/antlir2/facebook/images/build_appliance/centos9:build-appliance",
}

def rpm2extents(
        ctx: AnalysisContext,
        antlir2_isolate: RunInfo,
        rpm: Artifact,
        extents: Artifact,
        build_appliance: LayerInfo,
        identifier: str | None = None):
    ctx.actions.run(
        cmd_args(
            antlir2_isolate,
            cmd_args(build_appliance.subvol_symlink),
            cmd_args(rpm, format = "--input={}"),
            cmd_args(extents.as_output(), format = "--create-output-file={}"),
            "--",
            "/__antlir2__/dnf/rpm2extents",
            rpm,
            extents.as_output(),
        ),
        category = "rpm2extents",
        identifier = identifier,
        local_only = True,  # local subvolume required
        allow_cache_upload = True,  # the actual produced artifact is fine to cache
    )
