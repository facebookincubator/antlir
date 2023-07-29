# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":btrfs.bzl", "btrfs")
load(":sendstream.bzl", "sendstream", "sendstream_v2", "sendstream_zst")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    extension = {
        "cpio.gz": ".cpio.gz",
        "cpio.zst": ".cpio.zst",
        "rpm": ".rpm",
        "squashfs": ".squashfs",
        "vfat": ".vfat",
    }[ctx.attrs.format]
    package = ctx.actions.declare_output("image" + extension)

    spec_opts = {
        "build_appliance": (ctx.attrs.build_appliance or ctx.attrs.layer[LayerInfo].build_appliance)[LayerInfo].subvol_symlink,
        "layer": ctx.attrs.layer[LayerInfo].subvol_symlink,
    }
    spec_opts.update(ctx.attrs.opts)

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
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "format": attrs.enum(["cpio.gz", "cpio.zst", "vfat", "rpm", "squashfs"]),
        "layer": attrs.dep(providers = [LayerInfo]),
        "opts": attrs.dict(attrs.string(), attrs.any(), default = {}, doc = "options for this package format"),
    },
)

_package_macro = rule_with_default_target_platform(_package)

def check_kwargs(kwargs):
    if "opts" in kwargs:
        fail("opts is not allowed to be provided as kwargs")

def _cpio_gz(
        name: str,
        layer: str,
        compression_level: int = 3,
        **kwargs):
    check_kwargs(kwargs)
    return _package_macro(
        name = name,
        layer = layer,
        format = "cpio.gz",
        opts = {
            "compression_level": compression_level,
        },
        **kwargs
    )

def _cpio_zst(
        name: str,
        layer: str,
        compression_level: int = 15,
        **kwargs):
    check_kwargs(kwargs)
    return _package_macro(
        name = name,
        layer = layer,
        format = "cpio.zst",
        opts = {
            "compression_level": compression_level,
        },
        **kwargs
    )

def _rpm(
        name: str,
        layer: str,
        rpm_name: str,
        version: str,
        release: str,
        arch: str,
        license: str,
        epoch: int = 0,
        summary: str | None = None,
        requires: list[str] = [],
        recommends: list[str] = [],
        provides: list[str] = [],
        supplements: list[str] = [],
        conflicts: list[str] = [],
        description: str | None = None,
        post_install_script: str | None = None,
        **kwargs):
    check_kwargs(kwargs)

    opts = {
        "arch": arch,
        "conflicts": conflicts,
        "description": description,
        "epoch": epoch,
        "license": license,
        "name": rpm_name,
        "post_install_script": post_install_script,
        "provides": provides,
        "recommends": recommends,
        "release": release,
        "requires": requires,
        "summary": summary or rpm_name,
        "supplements": supplements,
        "version": version,
    }

    return _package_macro(
        name = name,
        layer = layer,
        format = "rpm",
        opts = opts,
        **kwargs
    )

def _vfat(
        name: str,
        layer: str,
        fat_size: int | None = None,
        label: str | None = None,
        size_mb: int | None = None,
        **kwargs):
    check_kwargs(kwargs)

    opts = {}
    if fat_size != None:
        opts["fat_size"] = fat_size

    if label != None:
        opts["label"] = label

    if size_mb != None:
        opts["size_mb"] = size_mb

    return _package_macro(
        name = name,
        layer = layer,
        format = "vfat",
        opts = opts,
        **kwargs
    )

def _squashfs(
        name: str,
        layer: str,
        **kwargs):
    check_kwargs(kwargs)
    return _package_macro(
        name = name,
        layer = layer,
        format = "squashfs",
        **kwargs
    )

def _backwards_compatible_new(format: str, **kwargs):
    {
        "btrfs": btrfs,
        "cpio.gz": _cpio_gz,
        "cpio.zst": _cpio_zst,
        "rpm": _rpm,
        "sendstream": sendstream,
        "sendstream.v2": sendstream_v2,
        "sendstream.zst": sendstream_zst,
        "squashfs": _squashfs,
        "vfat": _vfat,
    }[format](**kwargs)

package = struct(
    backward_compatible_new = _backwards_compatible_new,
    cpio_gz = _cpio_gz,
    cpio_zst = _cpio_zst,
    btrfs = btrfs,
    rpm = _rpm,
    sendstream = sendstream,
    sendstream_v2 = sendstream_v2,
    sendstream_zst = sendstream_zst,
    squashfs = _squashfs,
    vfat = _vfat,
)
