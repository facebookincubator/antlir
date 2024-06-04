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
    "LayerContents",  # @unused Used as type
)

def _new_facts_db(
        *,
        actions: AnalysisActions,
        layer: LayerContents,
        parent_facts_db: Artifact | None,
        build_appliance: BuildApplianceInfo | Provider | None,
        new_facts_db: RunInfo,
        phase: BuildPhase | None,
        rootless: bool) -> Artifact:
    prefix = phase.value if phase else None
    if prefix:
        output = actions.declare_output(prefix, "facts")
    else:
        output = actions.declare_output("facts")

    # Inspecting already-built images often requires root privileges
    sudo = True
    if rootless:  # rootless builds must avoid sudo
        sudo = False
    if layer.overlayfs:  # overlayfs layers have metadata accessible without root
        sudo = False

    actions.run(
        cmd_args(
            "sudo" if sudo else cmd_args(),
            new_facts_db,
            cmd_args(layer.subvol_symlink, format = "--subvol-symlink={}") if layer.subvol_symlink else cmd_args(),
            cmd_args(layer.overlayfs.json_file_with_inputs, format = "--overlayfs={}") if layer.overlayfs else cmd_args(),
            cmd_args(parent_facts_db, format = "--parent={}") if parent_facts_db else cmd_args(),
            cmd_args(build_appliance.dir, format = "--build-appliance={}") if build_appliance else cmd_args(),
            cmd_args(output.as_output(), format = "--db={}"),
            "--rootless" if rootless else cmd_args(),
        ),
        category = "antlir2_facts",
        identifier = prefix,
        # needs local subvol if not overlayfs
        local_only = not layer.overlayfs,
        env = {
            "RUST_LOG": "populate=trace",
        },
    )
    return output

facts = struct(
    new_facts_db = _new_facts_db,
)
