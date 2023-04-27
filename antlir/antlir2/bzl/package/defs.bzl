# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

def _detect_build_appliance(layer, build_appliance):
    if build_appliance != None:
        return build_appliance[LayerInfo]

    flavor_info = layer[LayerInfo].flavor_info
    return flavor_info.default_build_appliance[LayerInfo]

def _impl(ctx: "context") -> ["provider"]:
    extension = {
        "btrfs": ".btrfs",
        "cpio.gz": ".cpio.gz",
        "cpio.zst": ".cpio.zst",
        "rpm": ".rpm",
        "sendstream.v2": ".sendstream.v2",
        "sendstream.zst": ".sendstream.zst",
        "vfat": ".vfat",
    }[ctx.attrs.format]
    package = ctx.actions.declare_output("image" + extension)

    if "layer" in ctx.attrs.opts:
        fail("Layer must not be provided in attrs.opts")
    if "subvols" in ctx.attrs.opts:
        fail("Subvols must not be provided in attrs.opts")

    spec_opts = {}
    if ctx.attrs.format == "btrfs":
        spec_opts["btrfs_packager_path"] = ctx.attrs.btrfs_packager[RunInfo]
        spec_opts["spec"] = {}
        spec_opts["spec"].update(ctx.attrs.opts)
        if ctx.attrs.subvols == None:
            fail("subvols must be provided for all non-btrfs formats")

        subvols = {}
        for path, subvol in ctx.attrs.subvols.items():
            subvols[path] = {
                "layer": subvol["layer"][LayerInfo].subvol_symlink,
                "writable": subvol.get("writable"),
            }
        spec_opts["spec"]["subvols"] = subvols
    else:
        spec_opts.update(ctx.attrs.opts)
        if ctx.attrs.layer == None:
            fail("layer must be provided for all non-btrfs formats")

        spec_opts["build_appliance"] = _detect_build_appliance(
            layer = ctx.attrs.layer,
            build_appliance = ctx.attrs.build_appliance,
        ).subvol_symlink

        spec_opts["layer"] = ctx.attrs.layer[LayerInfo].subvol_symlink

    spec = ctx.actions.write_json("spec.json", {ctx.attrs.format: spec_opts}, with_inputs = True)
    ctx.actions.run(
        cmd_args(
            ctx.attrs.antlir2_packager[RunInfo],
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
        "antlir2_packager": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_package/antlir2_packager:antlir2-packager")),
        "btrfs_packager": attrs.default_only(attrs.dep(providers = [RunInfo], default = "//antlir/antlir2/antlir2_package/btrfs_packager:btrfs-packager")),
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "format": attrs.enum(["btrfs", "sendstream.v2", "sendstream.zst", "cpio.gz", "cpio.zst", "vfat", "rpm"]),
        "layer": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "opts": attrs.dict(attrs.string(), attrs.any(), default = {}, doc = "options for this package format"),
        "subvols": attrs.option(
            attrs.dict(
                attrs.string(),
                attrs.dict(
                    attrs.string(),
                    attrs.option(
                        attrs.one_of(
                            attrs.dep(providers = [LayerInfo]),
                            attrs.bool(),
                        ),
                    ),
                ),
            ),
            default = None,
        ),
    },
)

def BtrfsSubvol(
        layer: str.type,
        writable: [bool.type, None] = None):
    return {
        "layer": layer,
        "writable": writable,
    }

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
        subvols = None,
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

def _btrfs(
        name: str.type,
        subvols: dict.type,
        default_subvol: str.type,
        free_mb: [int.type, None] = None,
        compression_level: int.type = 3,
        label: [str.type, None] = None,
        **kwargs):
    check_kwargs(kwargs)
    return _package(
        name = name,
        format = "btrfs",
        subvols = subvols,
        opts = {
            "compression_level": compression_level,
            "default_subvol": default_subvol,
            "free_mb": free_mb,
            "label": label,
        },
        **kwargs
    )

def _rpm(
        name: str.type,
        layer: str.type,
        rpm_name: str.type,
        version: str.type,
        release: str.type,
        arch: str.type,
        license: str.type,
        epoch: int.type = 0,
        summary: [str.type, None] = None,
        requires: [str.type] = [],
        recommends: [str.type] = [],
        **kwargs):
    check_kwargs(kwargs)

    opts = {
        "arch": arch,
        "epoch": epoch,
        "license": license,
        "name": rpm_name,
        "recommends": recommends,
        "release": release,
        "requires": requires,
        "summary": summary or rpm_name,
        "version": version,
    }

    return _package(
        name = name,
        layer = layer,
        format = "rpm",
        subvols = None,
        opts = opts,
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
        subvols = None,
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
        subvols = None,
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
        subvols = None,
        opts = opts,
        **kwargs
    )

package = struct(
    backward_compatible_new = _package,
    cpio_gz = _cpio_gz,
    cpio_zst = _cpio_zst,
    btrfs = _btrfs,
    rpm = _rpm,
    sendstream_v2 = _sendstream_v2,
    sendstream_zst = _sendstream_zst,
    vfat = _vfat,
)
