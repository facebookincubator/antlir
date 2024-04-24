# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "//antlir/antlir2/bzl:build_phase.bzl",
    "BuildPhase",  # @unused Used as type
)
load(
    "//antlir/antlir2/bzl:types.bzl",
    "BuildApplianceInfo",  # @unused Used as type
)

def _new_facts_db(
        *,
        actions: AnalysisActions,
        subvol_symlink: Artifact,
        build_appliance: BuildApplianceInfo | Provider | None,
        new_facts_db: RunInfo,
        phase: BuildPhase | None,
        rootless: bool) -> Artifact:
    prefix = phase.value if phase else None
    if prefix:
        output = actions.declare_output(
            prefix,
            "facts",
            dir = True,
        )
    else:
        output = actions.declare_output("facts", dir = True)
    actions.run(
        cmd_args(
            # Inspecting already-built images often requires root privileges
            "sudo" if not rootless else cmd_args(),
            new_facts_db,
            cmd_args(subvol_symlink, format = "--root={}"),
            cmd_args(build_appliance.dir, format = "--build-appliance={}") if build_appliance else cmd_args(),
            cmd_args(output.as_output(), format = "--db={}"),
            "--rootless" if rootless else cmd_args(),
        ),
        category = "antlir2_facts",
        identifier = prefix,
        local_only = True,  # needs local subvol
        env = {
            "RUST_LOG": "populate=trace",
        },
    )
    return output

facts = struct(
    new_facts_db = _new_facts_db,
)
