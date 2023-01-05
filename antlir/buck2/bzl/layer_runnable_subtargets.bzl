# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

layer_runtime_types = enum("container", "systemd")
layer_runtime_attr = attrs.list(attrs.enum(layer_runtime_types.values()), default = ["container"])

def layer_runnable_subtargets(nspawn_in_subvol_run: "dependency", runtime: [str.type], layer_out_dir: "artifact"):
    # ensure that all the variants are valid values of this enum
    for r in runtime:
        layer_runtime_types(r)
    runtime_targets = {}
    runtime_targets["container"] = [
        RunInfo(cmd_args(
            nspawn_in_subvol_run[RunInfo],
            "--layer",
            layer_out_dir,
        )),
        DefaultInfo(default_outputs = []),
    ]
    if "systemd" in runtime:
        runtime_targets["systemd"] = [
            RunInfo(cmd_args(
                nspawn_in_subvol_run[RunInfo],
                "--layer",
                layer_out_dir,
                "--boot",
                "--append-console",
            )),
            DefaultInfo(default_outputs = []),
        ]
    return runtime_targets

def make_alias_with_equals_suffix(layer_name: str.type, runtime: [str.type]):
    if "container" not in runtime:
        runtime += ["container"]
    for r in runtime:
        # ensure that all the variants are valid values of this enum
        layer_runtime_types(r)

        native.alias(
            name = layer_name + "=" + r,
            actual = ":{}[{}]".format(layer_name, r),
        )
