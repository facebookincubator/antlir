# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/appliance_vm:defs.bzl", "ApplianceVmInfo")
load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg", "cfg_attrs")
load("//antlir/antlir2/bzl/package:cfg.bzl", "package_cfg")
load(":gpt.bzl", "GptPartitionSource")
load(":macro.bzl", "package_macro")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    package = ctx.actions.declare_output("image.btrfs")
    build_appliance = ctx.attrs.build_appliance

    spec = ctx.actions.write_json(
        "spec.json",
        {"btrfs": {
            "build_appliance": build_appliance[BuildApplianceInfo].dir,
            "compression_level": ctx.attrs.compression_level,
            "default_subvol": ctx.attrs.default_subvol,
            "free_mb": ctx.attrs.free_mb,
            "label": ctx.attrs.label,
            "seed_device": ctx.attrs.seed_device,
            "subvols": {
                path: {
                    "layer": subvol["layer"][LayerInfo].contents.subvol_symlink,
                    "writable": subvol.get("writable") or False,
                }
                for path, subvol in ctx.attrs.subvols.items()
            },
        }},
        with_inputs = True,
    )

    ctx.actions.run(
        cmd_args(
            "sudo" if not ctx.attrs._rootless else cmd_args(),
            ctx.attrs.antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            cmd_args(package.as_output(), format = "--out={}"),
            "--rootless" if ctx.attrs._rootless else cmd_args(),
        ),
        local_only = True,  # needs local subvolumes
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
        "appliance_vm": attrs.default_only(attrs.exec_dep(providers = [ApplianceVmInfo], default = "antlir//antlir/antlir2/appliance_vm:appliance_vm")),
        "build_appliance": attrs.option(attrs.exec_dep(providers = [BuildApplianceInfo]), default = None),
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
    } | attrs_selected_by_cfg() | cfg_attrs(),
    cfg = package_cfg,
)

btrfs = package_macro(_btrfs)

def BtrfsSubvol(
        layer: str | Select,
        writable: bool | None = None):
    return {
        "layer": layer,
        "writable": writable,
    }
