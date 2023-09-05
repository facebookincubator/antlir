# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @starlark-rust: allow_string_literals_in_type_expr

load("//antlir/antlir2/bzl:platform.bzl", "arch_select", "default_target_platform_kwargs", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load(":btrfs.bzl", "btrfs")
load(":gpt.bzl", "GptPartitionSource", "gpt")
load(":sendstream.bzl", "sendstream", "sendstream_v2", "sendstream_zst")
load(":stamp_buildinfo.bzl", "stamp_buildinfo_rule")

# Attrs that are required by all packages
_common_attrs = {
    "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
    "layer": attrs.dep(providers = [LayerInfo]),
}

# Attrs that will only ever be used as default_only
_default_attrs = {
    "_antlir2": attrs.exec_dep(default = "//antlir/antlir2/antlir2:antlir2"),
    "_antlir2_packager": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_packager:antlir2-packager")),
    "_dot_meta_feature": attrs.dep(default = "//antlir/antlir2/bzl/package:dot-meta"),
    "_objcopy": attrs.default_only(attrs.exec_dep(default = "fbsource//third-party/binutils:objcopy")),
    "_run_nspawn": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/nspawn_in_subvol:nspawn")),
    "_target_arch": attrs.default_only(attrs.string(
        default = arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
    )),
}

def _generic_impl_with_layer(
        layer: [Dependency, "provider_collection"],
        *,
        ctx: AnalysisContext,
        format: str,
        rule_attr_keys: list[str],
        can_be_partition: bool) -> list[Provider]:
    extension = {
        "cpio": ".cpio",
        "rpm": ".rpm",
        "squashfs": ".squashfs",
        "tar": ".tar",
        "vfat": ".vfat",
    }[format]
    package = ctx.actions.declare_output("package" + extension)

    build_appliance = ctx.attrs.build_appliance or layer[LayerInfo].build_appliance
    spec_opts = {
        "build_appliance": build_appliance[LayerInfo].subvol_symlink,
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
    providers = [DefaultInfo(package)]
    if can_be_partition:
        providers.append(GptPartitionSource(src = package))
    return providers

def _generic_impl(
        ctx: AnalysisContext,
        format: str,
        rule_attr_keys: list[str],
        dot_meta: bool,
        can_be_partition: bool):
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
            can_be_partition = can_be_partition,
        ))
    else:
        return _generic_impl_with_layer(
            layer = ctx.attrs.layer,
            ctx = ctx,
            format = format,
            rule_attr_keys = rule_attr_keys,
            can_be_partition = can_be_partition,
        )

# Create a new buck2 rule that implements a specific package format.
def _new_package_rule(
        format: str,
        rule_attrs: dict[str, "attribute"] = {},
        dot_meta: bool = True,
        can_be_partition = False):
    return rule(
        impl = partial(
            _generic_impl,
            format = format,
            rule_attr_keys = list(rule_attrs.keys()),
            dot_meta = dot_meta,
            can_be_partition = can_be_partition,
        ),
        attrs = _default_attrs | _common_attrs | rule_attrs,
    )

def _compressed_impl(
        ctx: AnalysisContext,
        uncompressed: typing.Callable,
        rule_attr_keys: list[str],
        compressor: str) -> list[Provider]:
    src = ctx.actions.artifact_promise(ctx.actions.anon_target(
        uncompressed,
        {key: getattr(ctx.attrs, key) for key in _default_attrs.keys()} |
        {
            "layer": ctx.attrs.layer,
            "name": str(ctx.label.raw_target()),
        } | {key: getattr(ctx.attrs, key) for key in rule_attr_keys},
    ).map(lambda x: ensure_single_output(x)))
    extension = {
        "gzip": ".gz",
        "zstd": ".zst",
    }[compressor]
    package = ctx.actions.declare_output("package" + extension)

    if compressor == "gzip":
        compress_cmd = cmd_args(
            "gzip",
            cmd_args(str(ctx.attrs.compression_level), format = "-{}"),
            src,
            cmd_args(package.as_output(), format = "--stdout > {}"),
            delimiter = " \\\n",
        )
    elif compressor == "zstd":
        compress_cmd = cmd_args(
            "zstd",
            "--compress",
            cmd_args(str(ctx.attrs.compression_level), format = "-{}"),
            "-T0",  # we like threads
            src,
            cmd_args(package.as_output(), format = "--stdout > {}"),
            delimiter = " \\\n",
        )
    else:
        fail("unknown compressor '{}'".format(compressor))

    script = ctx.actions.write(
        "compress.sh",
        cmd_args(
            "#!/bin/sh",
            compress_cmd,
            delimiter = "\n",
        ),
        is_executable = True,
    )
    ctx.actions.run(cmd_args("/bin/sh", script).hidden(package.as_output(), src), category = "compress")
    return [DefaultInfo(package)]

def _new_compressed_package_rule(
        compressor: str,
        uncompressed: typing.Callable,
        default_compression_level: int,
        rule_attrs: dict[str, "attribute"] = {}):
    return rule(
        impl = partial(
            _compressed_impl,
            uncompressed = uncompressed,
            rule_attr_keys = list(rule_attrs.keys()),
            compressor = compressor,
        ),
        attrs = _default_attrs | _common_attrs | rule_attrs | {
            "compression_level": attrs.int(default = default_compression_level),
        },
    )

_cpio = _new_package_rule(
    format = "cpio",
)

_cpio_gz = _new_compressed_package_rule(
    default_compression_level = 3,
    uncompressed = _cpio,
    compressor = "gzip",
)

_cpio_zst = _new_compressed_package_rule(
    default_compression_level = 15,
    uncompressed = _cpio,
    compressor = "zstd",
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
    can_be_partition = True,
)

_squashfs = _new_package_rule(
    rule_attrs = {},
    format = "squashfs",
    can_be_partition = True,
)

_tar = _new_package_rule(
    format = "tar",
)

_tar_gz = _new_compressed_package_rule(
    default_compression_level = 3,
    uncompressed = _tar,
    compressor = "gzip",
)

_tar_zst = _new_compressed_package_rule(
    default_compression_level = 15,
    uncompressed = _tar,
    compressor = "zstd",
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
        "tar.zst": _tar_zst,
        "vfat": _vfat,
    }[format](
        **(default_target_platform_kwargs() | kwargs)
    )

package = struct(
    backward_compatible_new = _backwards_compatible_new,
    btrfs = btrfs,
    cpio_gz = rule_with_default_target_platform(_cpio_gz),
    cpio_zst = rule_with_default_target_platform(_cpio_zst),
    gpt = gpt,
    rpm = rule_with_default_target_platform(_rpm),
    sendstream = sendstream,
    sendstream_v2 = sendstream_v2,
    sendstream_zst = sendstream_zst,
    squashfs = rule_with_default_target_platform(_squashfs),
    tar_gz = rule_with_default_target_platform(_tar_gz),
    tar_zst = rule_with_default_target_platform(_tar_zst),
    vfat = rule_with_default_target_platform(_vfat),
)
