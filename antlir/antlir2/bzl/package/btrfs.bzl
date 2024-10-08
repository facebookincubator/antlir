# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/package:cfg.bzl", "cfg_attrs", "package_cfg")
load(":gpt.bzl", "GptPartitionSource")
load(":macro.bzl", "package_macro")
load(":sendstream.bzl", "sendstream_v2_anon")

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
                "seed_device": ctx.attrs.seed_device,
                "subvols": {
                    path: {
                        "sendstream": ctx.actions.anon_target(sendstream_v2_anon, {
                            "antlir2_packager": ctx.attrs.antlir2_packager,
                            "compression_level": ctx.attrs.compression_level,
                            "layer": subvol["layer"],
                            "name": str(subvol["layer"].label.raw_target()),
                            "_rootless": ctx.attrs._rootless,
                        }).artifact("sendstream"),
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
        identifier = "btrfs",
    )
    return [
        DefaultInfo(package),
        GptPartitionSource(src = package),
    ]

_btrfs = rule(
    impl = _impl,
    attrs = {
        "antlir2_packager": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/antlir2_packager:antlir2-packager")),
        "btrfs_packager": attrs.default_only(attrs.exec_dep(providers = [RunInfo], default = "antlir//antlir/antlir2/antlir2_packager/btrfs_packager:btrfs-packager")),
        "compression_level": attrs.int(default = 3),
        # used by transition
        "default_os": attrs.option(attrs.string(), default = None),
        "default_subvol": attrs.option(attrs.string(), default = None),
        "free_mb": attrs.option(attrs.int(), default = None),
        "label": attrs.option(attrs.string(), default = None),
        "labels": attrs.list(attrs.string(), default = []),
        "seed_device": attrs.bool(default = False),
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
        "_rootless": rootless_cfg.is_rootless_attr,
    } | cfg_attrs(),
    cfg = package_cfg,
)

btrfs = package_macro(_btrfs, always_needs_root = True)

def BtrfsSubvol(
        layer: str | Select,
        writable: bool | None = None):
    return {
        "layer": layer,
        "writable": writable,
    }
