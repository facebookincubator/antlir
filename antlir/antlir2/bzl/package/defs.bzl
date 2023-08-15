# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "default_target_platform_kwargs", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":btrfs.bzl", "btrfs")
load(":sendstream.bzl", "sendstream", "sendstream_v2", "sendstream_zst")
load(":stamp_buildinfo.bzl", "stamp_buildinfo_rule")

# Attrs that are required by all packages
_common_attrs = {
    "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
    "layer": attrs.dep(providers = [LayerInfo]),
}

_PACKAGER = "//antlir/antlir2/antlir2_package/antlir2_packager:antlir2-packager"

# Attrs that will only ever be used as default_only
_default_attrs = {
    "_antlir2": attrs.exec_dep(default = "//antlir/antlir2/antlir2:antlir2"),
    "_antlir2_packager": attrs.default_only(attrs.exec_dep(default = _PACKAGER)),
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
}

def _generic_impl_with_layer(
        layer: [Dependency, "provider_collection"],
        *,
        ctx: AnalysisContext,
        format: str,
        rule_attr_keys: list[str]) -> list[Provider]:
    extension = {
        "cpio.gz": ".cpio.gz",
        "cpio.zst": ".cpio.zst",
        "rpm": ".rpm",
        "squashfs": ".squashfs",
        "tar.gz": ".tar.gz",
        "vfat": ".vfat",
    }[format]
    package = ctx.actions.declare_output("image" + extension)

    spec_opts = {
        "build_appliance": (ctx.attrs.build_appliance or layer[LayerInfo].build_appliance)[LayerInfo].subvol_symlink,
        "layer": layer[LayerInfo].subvol_symlink,
    }
    for key in rule_attr_keys:
        spec_opts[key] = getattr(ctx.attrs, key)
    spec = ctx.actions.write_json("spec.json", {format: spec_opts}, with_inputs = True)
    ctx.actions.run(
        cmd_args(
            ctx.attrs._antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            cmd_args(package.as_output(), format = "--out={}"),
        ),
        local_only = True,
        category = "antlir2_package",
    )
    return [DefaultInfo(package)]

def _generic_impl(
        ctx: AnalysisContext,
        format: str,
        rule_attr_keys: list[str],
        dot_meta: bool):
    if dot_meta:
        return ctx.actions.anon_target(stamp_buildinfo_rule, {
            "layer": ctx.attrs.layer,
            "name": str(ctx.label.raw_target()),
            "_antlir2": ctx.attrs._antlir2,
            "_dot_meta_feature": ctx.attrs._dot_meta_feature,
            "_objcopy": ctx.attrs._objcopy,
            "_run_nspawn": ctx.attrs._run_nspawn,
            "_target_arch": ctx.attrs._target_arch,
        }).map(partial(
            _generic_impl_with_layer,
            ctx = ctx,
            format = format,
            rule_attr_keys = rule_attr_keys,
        ))
    else:
        return _generic_impl_with_layer(
            layer = ctx.attrs.layer,
            ctx = ctx,
            format = format,
            rule_attr_keys = rule_attr_keys,
        )

# Create a new buck2 rule that implements a specific package format.
def _new_package_rule(
        format: str,
        rule_attrs: dict[str, "attribute"] = {},
        dot_meta: bool = True):
    return rule(
        impl = partial(
            _generic_impl,
            format = format,
            rule_attr_keys = list(rule_attrs.keys()),
            dot_meta = dot_meta,
        ),
        attrs = _default_attrs | _common_attrs | rule_attrs,
    )

_cpio_gz = _new_package_rule(
    rule_attrs = {
        "compression_level": attrs.int(default = 3),
    },
    format = "cpio.gz",
)

_cpio_zst = _new_package_rule(
    rule_attrs = {
        "compression_level": attrs.int(default = 15),
    },
    format = "cpio.zst",
)

_rpm = _new_package_rule(
    rule_attrs = {
        "arch": attrs.string(),
        "conflicts": attrs.list(attrs.string(), default = []),
        "description": attrs.option(attrs.string(), default = None),
        "epoch": attrs.int(default = 0),
        "license": attrs.string(),
        "post_install_script": attrs.option(attrs.string(), default = None),
        "provides": attrs.list(attrs.string(), default = []),
        "recommends": attrs.list(attrs.string(), default = []),
        "release": attrs.string(),
        "requires": attrs.list(attrs.string(), default = []),
        "rpm_name": attrs.string(),
        "summary": attrs.option(attrs.string(), default = None),
        "supplements": attrs.list(attrs.string(), default = []),
        "version": attrs.string(),
    },
    format = "rpm",
    dot_meta = False,
)

_vfat = _new_package_rule(
    rule_attrs = {
        "fat_size": attrs.option(attrs.int(), default = None),
        "label": attrs.option(attrs.string(), default = None),
        "size_mb": attrs.option(attrs.int(), default = None),
    },
    format = "vfat",
)

_squashfs = _new_package_rule(rule_attrs = {}, format = "squashfs")

_tar_gz = _new_package_rule(
    rule_attrs = {
        "compression_level": attrs.int(default = 15),
    },
    format = "tar.gz",
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
    }[format](
        **(default_target_platform_kwargs() | kwargs)
    )

package = struct(
    backward_compatible_new = _backwards_compatible_new,
    cpio_gz = rule_with_default_target_platform(_cpio_gz),
    cpio_zst = rule_with_default_target_platform(_cpio_zst),
    btrfs = btrfs,
    rpm = rule_with_default_target_platform(_rpm),
    sendstream = sendstream,
    sendstream_v2 = sendstream_v2,
    sendstream_zst = sendstream_zst,
    squashfs = rule_with_default_target_platform(_squashfs),
    tar_gz = rule_with_default_target_platform(_tar_gz),
    vfat = rule_with_default_target_platform(_vfat),
)
