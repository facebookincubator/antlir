# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":btrfs.bzl", "btrfs")
load(":sendstream.bzl", "sendstream", "sendstream_v2", "sendstream_zst")
load(":stamp_buildinfo.bzl", "stamp_buildinfo_rule")

def _impl_with_layer(layer: [Dependency, "provider_collection"], *, ctx: AnalysisContext) -> list[Provider]:
    extension = {
        "cpio.gz": ".cpio.gz",
        "cpio.zst": ".cpio.zst",
        "rpm": ".rpm",
        "squashfs": ".squashfs",
        "tar.gz": ".tar.gz",
        "vfat": ".vfat",
    }[ctx.attrs.format]
    package = ctx.actions.declare_output("image" + extension)

    spec_opts = {
        "build_appliance": (ctx.attrs.build_appliance or layer[LayerInfo].build_appliance)[LayerInfo].subvol_symlink,
        "layer": layer[LayerInfo].subvol_symlink,
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

def _impl(ctx: AnalysisContext):
    if ctx.attrs.dot_meta:
        return ctx.actions.anon_target(stamp_buildinfo_rule, {
            "layer": ctx.attrs.layer,
            "name": str(ctx.label.raw_target()),
            "_antlir2": ctx.attrs._antlir2,
            "_dot_meta_feature": ctx.attrs._dot_meta_feature,
            "_objcopy": ctx.attrs._objcopy,
            "_run_nspawn": ctx.attrs._run_nspawn,
            "_target_arch": ctx.attrs._target_arch,
        }).map(partial(_impl_with_layer, ctx = ctx))
    else:
        return _impl_with_layer(
            layer = ctx.attrs.layer,
            ctx = ctx,
        )

_package = rule(
    impl = _impl,
    attrs = {
        "antlir2_packager": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_package/antlir2_packager:antlir2-packager")),
        "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "dot_meta": attrs.bool(default = True, doc = "record build info in /.meta"),
        "format": attrs.enum(["cpio.gz", "cpio.zst", "vfat", "rpm", "squashfs", "tar.gz"]),
        "layer": attrs.dep(providers = [LayerInfo]),
        "opts": attrs.dict(attrs.string(), attrs.any(), default = {}, doc = "options for this package format"),
        "_antlir2": attrs.exec_dep(default = "//antlir/antlir2/antlir2:antlir2"),
        "_dot_meta_feature": attrs.dep(default = "//antlir/antlir2/bzl/package:dot-meta"),
        "_objcopy": attrs.default_only(attrs.exec_dep(default = "fbsource//third-party/binutils:objcopy")),
        "_run_nspawn": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/nspawn_in_subvol:nspawn")),
        "_target_arch": attrs.default_only(attrs.string(
            default =
                select({
                    "ovr_config//cpu:arm64": "aarch64",
                    "ovr_config//cpu:x86_64": "x86_64",
                }),
        )),
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
        dot_meta = False,
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

def _tar_gz(
        name: str,
        layer: str,
        compression_level: int = 3,
        **kwargs):
    check_kwargs(kwargs)
    opts = {
        "compression_level": compression_level,
    }
    return _package_macro(
        name = name,
        layer = layer,
        format = "tar.gz",
        opts = opts,
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
        "tar.gz": _tar_gz,
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
    tar_gz = _tar_gz,
    vfat = _vfat,
)
