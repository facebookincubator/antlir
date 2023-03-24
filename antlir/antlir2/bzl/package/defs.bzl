# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

def _impl(ctx: "context") -> ["provider"]:
    flavor_info = ctx.attrs.layer[LayerInfo].flavor_info
    build_appliance = (ctx.attrs.build_appliance or flavor_info.default_build_appliance)[LayerInfo]

    extension = {"cpio.gz": ".cpio.gz", "cpio.zst": ".cpio.zst", "sendstream.v2": ".sendstream.v2", "sendstream.zst": ".sendstream.zst", "vfat": ".vfat"}[ctx.attrs.format]
    package = ctx.actions.declare_output("image" + extension)

    spec_opts = {}
    spec_opts.update(ctx.attrs.opts)
    if "layer" in spec_opts:
        fail("Layer must not be provided in attrs.opts")
    spec_opts["layer"] = ctx.attrs.layer[LayerInfo].subvol_symlink

    spec = ctx.actions.write_json("spec.json", {ctx.attrs.format: spec_opts}, with_inputs = True)
    ctx.actions.run(
        cmd_args(
            ctx.attrs.antlir2_package[RunInfo],
            cmd_args(build_appliance.subvol_symlink, format = "--build-appliance={}"),
            cmd_args(spec, format = "--spec={}"),
            cmd_args(package.as_output(), format = "--out={}"),
        ),
        local_only = True,
        category = "antlir2_package",
    )
    return [DefaultInfo(package)]

_package = rule(
    impl = _impl,
    attrs = {
        "antlir2_package": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_package:antlir2-package")),
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "format": attrs.enum(["sendstream.v2", "sendstream.zst", "cpio.gz", "cpio.zst", "vfat"]),
        "layer": attrs.dep(providers = [LayerInfo]),
        "opts": attrs.dict(attrs.string(), attrs.any(), default = {}, doc = "options for this package format"),
    },
)

def check_kwargs(kwargs):
    if "opts" in kwargs:
        fail("opts is not allowed to be provided as kwargs")

def _cpio_gz(
        name: str.type,
        layer: str.type,
        compression_level: int.type = 3,
        **kwargs):
    check_kwargs(kwargs)
    return _package(
        name = name,
        layer = layer,
        format = "cpio.gz",
        opts = {
            "compression_level": compression_level,
        },
        **kwargs
    )

def _cpio_zst(
        name: str.type,
        layer: str.type,
        compression_level: int.type = 15,
        **kwargs):
    check_kwargs(kwargs)
    return _package(
        name = name,
        layer = layer,
        format = "cpio.zst",
        opts = {
            "compression_level": compression_level,
        },
        **kwargs
    )

def _sendstream_v2(
        name: str.type,
        layer: str.type,
        compression_level: int.type = 3,
        **kwargs):
    check_kwargs(kwargs)
    return _package(
        name = name,
        layer = layer,
        format = "sendstream.v2",
        opts = {
            "compression_level": compression_level,
        },
        **kwargs
    )

def _sendstream_zst(
        name: str.type,
        layer: str.type,
        compression_level: int.type = 3,
        **kwargs):
    check_kwargs(kwargs)
    return _package(
        name = name,
        layer = layer,
        format = "sendstream.zst",
        opts = {
            "compression_level": compression_level,
        },
        **kwargs
    )

def _vfat(
        name: str.type,
        layer: str.type,
        fat_size: [int.type, None] = None,
        label: [str.type, None] = None,
        size_mb: [int.type, None] = None,
        **kwargs):
    check_kwargs(kwargs)

    opts = {}
    if fat_size != None:
        opts["fat_size"] = fat_size

    if label != None:
        opts["label"] = label

    if size_mb != None:
        opts["size_mb"] = size_mb

    return _package(
        name = name,
        layer = layer,
        format = "vfat",
        opts = opts,
        **kwargs
    )

package = struct(
    backward_compatible_new = _package,
    cpio_gz = _cpio_gz,
    cpio_zst = _cpio_zst,
    sendstream_v2 = _sendstream_v2,
    sendstream_zst = _sendstream_zst,
    vfat = _vfat,
)
