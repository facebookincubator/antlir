# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_overlayfs:overlayfs.bzl", "OverlayFs", "OverlayLayer")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")

TestLayerInfo = provider(fields = {
    "overlayfs": OverlayFs,
})

def _test_layer_impl(ctx: AnalysisContext) -> list[Provider]:
    data_dir = ctx.actions.declare_output("data_dir")
    manifest = ctx.actions.declare_output("manifest.json")
    parent = ctx.attrs.parent[TestLayerInfo].overlayfs if ctx.attrs.parent else None
    parent_layers = (parent.layers + [parent.top]) if parent else []
    fs = struct(
        top = OverlayLayer(
            data_dir = data_dir.as_output(),
            manifest = manifest.as_output(),
        ),
        layers = parent_layers,
    )
    model = ctx.actions.write_json("model-out.json", fs, with_inputs = True)
    ctx.actions.run(
        cmd_args(
            ctx.attrs._make_layer[RunInfo],
            cmd_args(model, format = "--model={}"),
            cmd_args(ctx.attrs.bash, format = "--bash={}"),
        ),
        category = "make_layer",
        # This obviously can work locally, but let's just set prefer_remote to
        # prove that this is viable for RE image builds
        prefer_remote = True,
    )

    fs = struct(
        top = OverlayLayer(
            data_dir = data_dir,
            manifest = manifest,
        ),
        layers = parent_layers,
    )
    model_json = ctx.actions.declare_output("model.json")
    model = ctx.actions.write_json(model_json, fs, with_inputs = True)

    subvol_symlink = ctx.actions.declare_output("subvol_symlink")
    ctx.actions.run(
        cmd_args(
            ctx.attrs._materialize_to_subvol[RunInfo],
            cmd_args(model, format = "--model={}"),
            cmd_args(subvol_symlink.as_output(), format = "--subvol-symlink={}"),
        ),
        category = "materialize_subvol",
        local_only = True,
    )

    return [
        DefaultInfo(sub_targets = {
            "data_dir": [DefaultInfo(data_dir)],
            "manifest": [DefaultInfo(manifest)],
            "subvol_symlink": [DefaultInfo(subvol_symlink)],
        }),
        TestLayerInfo(
            overlayfs = OverlayFs(
                top = OverlayLayer(
                    data_dir = data_dir,
                    manifest = manifest,
                ),
                layers = parent_layers,
                json_file = model_json,
                json_file_with_inputs = model,
            ),
        ),
    ]

_test_layer = rule(
    impl = _test_layer_impl,
    attrs = {
        "bash": attrs.arg(),
        "parent": attrs.option(attrs.dep(providers = [TestLayerInfo]), default = None),
        "_make_layer": attrs.exec_dep(default = "//antlir/antlir2/antlir2_overlayfs/tests:make-layer"),
        "_materialize_to_subvol": attrs.exec_dep(default = "//antlir/antlir2/antlir2_overlayfs:materialize-to-subvol"),
    },
)

test_layer = rule_with_default_target_platform(_test_layer)

def _overlay_sh_test_impl(ctx: AnalysisContext) -> list[Provider]:
    model = ctx.actions.write_json("model.json", ctx.attrs.layer[TestLayerInfo].overlayfs, with_inputs = True)
    test_cmd = [
        ctx.attrs._run_test[RunInfo],
        cmd_args(model, format = "--model={}"),
        cmd_args(ctx.attrs.bash, format = "--bash={}"),
    ]
    test_sh = ctx.actions.declare_output("test.sh")
    ctx.actions.write(
        test_sh,
        cmd_args(
            "#!/bin/bash -e",
            cmd_args(test_cmd, delimiter = " ", quote = "shell"),
            "\n",
            delimiter = "\n",
        ),
        allow_args = True,
        is_executable = True,
    )
    return [
        DefaultInfo(test_sh),
        RunInfo(cmd_args(test_cmd)),
        ExternalRunnerTestInfo(
            type = "simple",
            command = test_cmd,
            labels = ctx.attrs.labels,
        ),
    ]

_overlay_sh_test = rule(
    impl = _overlay_sh_test_impl,
    attrs = {
        "bash": attrs.arg(),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [TestLayerInfo]),
        "_run_test": attrs.exec_dep(default = "//antlir/antlir2/antlir2_overlayfs/tests:run-test"),
    },
)

overlay_sh_test = rule_with_default_target_platform(_overlay_sh_test)
