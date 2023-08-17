# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":sendstream.bzl", "anon_v1_sendstream")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    package = ctx.actions.declare_output("image.btrfs")

    spec = ctx.actions.write_json(
        "spec.json",
        {"btrfs": {
            "btrfs_packager_path": ctx.attrs.btrfs_packager[RunInfo],
            "spec": {
                "compression_level": ctx.attrs.compression_level,
                "default_subvol": ctx.attrs.default_subvol,
                "free_mb": ctx.attrs.free_mb,
                "label": ctx.attrs.label,
                "subvols": {
                    path: {
                        # needs access to the layer for size calculations :(
                        "layer": subvol["layer"][LayerInfo].subvol_symlink,
                        "sendstream": anon_v1_sendstream(
                            ctx = ctx,
                            layer = subvol["layer"],
                            build_appliance = ctx.attrs.build_appliance,
                        ),
                        "writable": subvol.get("writable"),
                    }
                    for path, subvol in ctx.attrs.subvols.items()
                },
            },
        }},
        with_inputs = True,
    )

    ctx.actions.run(
        cmd_args(
            ctx.attrs.antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            cmd_args(package.as_output(), format = "--out={}"),
        ),
        local_only = True,  # needs root
        category = "antlir2_package",
    )
    return [DefaultInfo(package)]

_btrfs = rule(
    impl = _impl,
    attrs = {
        "antlir2_packager": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_package/antlir2_packager:antlir2-packager")),
        "btrfs_packager": attrs.default_only(attrs.exec_dep(providers = [RunInfo], default = "//antlir/antlir2/antlir2_package/btrfs_packager:btrfs-packager")),
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "compression_level": attrs.int(default = 3),
        "default_subvol": attrs.string(),
        "free_mb": attrs.option(attrs.int(), default = None),
        "label": attrs.option(attrs.string(), default = None),
        "subvols": attrs.option(
            attrs.dict(
                attrs.string(doc = "subvol name"),
                attrs.dict(
                    attrs.string(),
                    attrs.option(
                        attrs.one_of(
                            attrs.dep(providers = [LayerInfo]),
                            attrs.bool(),
                        ),
                    ),
                    doc = "BtrfsSubvol()",
                ),
            ),
            default = None,
        ),
    },
)

btrfs = rule_with_default_target_platform(_btrfs)

def BtrfsSubvol(
        layer: str | Select,
        writable: bool | None = None):
    return {
        "layer": layer,
        "writable": writable,
    }
