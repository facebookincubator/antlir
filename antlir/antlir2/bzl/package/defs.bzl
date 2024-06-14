# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load(":btrfs.bzl", "btrfs")
load(":cfg.bzl", "layer_attrs", "package_cfg")
load(":gpt.bzl", "GptPartitionSource", "gpt")
load(":macro.bzl", "package_macro")
load(":sendstream.bzl", "sendstream_v2")
load(":stamp_buildinfo.bzl", "stamp_buildinfo_rule")

# Attrs that are required by all packages
common_attrs = {
    "build_appliance": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
    "out": attrs.option(attrs.string(doc = "Output filename"), default = None),
} | layer_attrs

# Attrs that will only ever be used as default_only
default_attrs = {
    "_analyze_feature": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2_depgraph_if:analyze")),
    "_antlir2": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2:antlir2")),
    "_antlir2_packager": attrs.default_only(attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2_packager:antlir2-packager"))),
    "_dot_meta_feature": attrs.dep(default = antlir2_dep("//antlir/antlir2/bzl/package:dot-meta")),
    "_new_facts_db": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/antlir2_facts:new-facts-db")),
    "_run_container": attrs.exec_dep(default = antlir2_dep("//antlir/antlir2/container_subtarget:run")),
    "_target_arch": attrs.default_only(attrs.string(
        default = arch_select(aarch64 = "aarch64", x86_64 = "x86_64"),
    )),
} | {k: attrs.default_only(v) for k, v in attrs_selected_by_cfg().items()}

def _generic_impl_with_layer(
        layer: [Dependency, ProviderCollection],
        *,
        ctx: AnalysisContext,
        format: str,
        rule_attr_keys: list[str],
        can_be_partition: bool,
        is_dir: bool,
        sudo: bool,
        force_extension: str | None) -> list[Provider]:
    build_appliance = ctx.attrs.build_appliance or layer[LayerInfo].build_appliance

    output_name = ctx.attrs.out or ctx.label.name
    if force_extension and not output_name.endswith("." + force_extension):
        output_name += "." + force_extension

    package = ctx.actions.declare_output(output_name, dir = is_dir)
    spec_opts = {
        "build_appliance": build_appliance[BuildApplianceInfo].dir,
        "layer": layer[LayerInfo].contents.subvol_symlink,
    }
    for key in rule_attr_keys:
        spec_opts[key] = getattr(ctx.attrs, key)

    spec = ctx.actions.write_json(
        "spec.json",
        {format: spec_opts},
        with_inputs = True,
    )
    ctx.actions.run(
        cmd_args(
            cmd_args("sudo", "--preserve-env=TMPDIR") if (sudo and not ctx.attrs._rootless) else cmd_args(),
            ctx.attrs._antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            "--dir" if is_dir else cmd_args(),
            cmd_args(package.as_output(), format = "--out={}"),
            "--rootless" if ctx.attrs._rootless else cmd_args(),
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
        can_be_partition: bool,
        is_dir: bool,
        sudo: bool,
        force_extension: str | None):
    if ctx.attrs.dot_meta:
        return ctx.actions.anon_target(stamp_buildinfo_rule, {
            "flavor": ctx.attrs.flavor,
            "layer": ctx.attrs.layer,
            "name": str(ctx.label.raw_target()),
            "_analyze_feature": ctx.attrs._analyze_feature,
            "_antlir2": ctx.attrs._antlir2,
            "_dot_meta_feature": ctx.attrs._dot_meta_feature,
            "_new_facts_db": ctx.attrs._new_facts_db,
            "_rootless": ctx.attrs._rootless,
            "_run_container": ctx.attrs._run_container,
            "_target_arch": ctx.attrs._target_arch,
        }).promise.map(partial(
            _generic_impl_with_layer,
            ctx = ctx,
            format = format,
            rule_attr_keys = rule_attr_keys,
            can_be_partition = can_be_partition,
            is_dir = is_dir,
            sudo = sudo,
            force_extension = force_extension,
        ))
    else:
        return _generic_impl_with_layer(
            layer = ctx.attrs.layer,
            ctx = ctx,
            format = format,
            rule_attr_keys = rule_attr_keys,
            can_be_partition = can_be_partition,
            is_dir = is_dir,
            sudo = sudo,
            force_extension = force_extension,
        )

# Create a new buck2 rule that implements a specific package format.
def _new_package_rule(
        format: str,
        rule_attrs: dict[str, Attr] = {},
        dot_meta: bool = True,
        can_be_partition: bool = False,
        is_dir: bool = False,
        sudo: bool = False,
        force_extension: str | None = None):
    kwargs = {
        "attrs": default_attrs | common_attrs | rule_attrs | {
            "dot_meta": attrs.bool(default = dot_meta),
        },
        "impl": partial(
            _generic_impl,
            format = format,
            rule_attr_keys = list(rule_attrs.keys()),
            can_be_partition = can_be_partition,
            is_dir = is_dir,
            sudo = sudo,
            force_extension = force_extension,
        ),
    }
    return (
        rule(
            cfg = package_cfg,
            **kwargs
        ),
        anon_rule(
            artifact_promise_mappings = {
                "package": lambda x: ensure_single_output(x),
            },
            **kwargs
        ),
    )

def _compressed_impl(
        ctx: AnalysisContext,
        uncompressed: typing.Callable,
        rule_attr_keys: list[str],
        compressor: str) -> list[Provider]:
    src = ctx.actions.anon_target(
        uncompressed,
        {key: getattr(ctx.attrs, key) for key in default_attrs.keys()} |
        {
            "layer": ctx.attrs.layer,
            "name": str(ctx.label.raw_target()),
            "out": "uncompressed",
        } | {key: getattr(ctx.attrs, key) for key in rule_attr_keys},
    ).artifact("package")
    package = ctx.actions.declare_output(ctx.label.name)

    if compressor == "gzip":
        compress_cmd = cmd_args(
            "compressor=\"$(which pigz || which gzip)\"",
            cmd_args(
                "$compressor",
                cmd_args(str(ctx.attrs.compression_level), format = "-{}"),
                src,
                cmd_args(package.as_output(), format = "--stdout > {}"),
                delimiter = " \\\n",
            ),
            delimiter = " \n",
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
    ctx.actions.run(cmd_args(
        "/bin/sh",
        script,
        hidden = [package.as_output(), src],
    ), category = "compress")
    return [DefaultInfo(package, sub_targets = {
        "uncompressed": [DefaultInfo(src)],
    })]

def _new_compressed_package_rule(
        compressor: str,
        uncompressed: typing.Callable,
        default_compression_level: int,
        rule_attrs: dict[str, Attr] = {}):
    return rule(
        impl = partial(
            _compressed_impl,
            uncompressed = uncompressed,
            rule_attr_keys = list(rule_attrs.keys()),
            compressor = compressor,
        ),
        attrs = default_attrs | common_attrs | rule_attrs | {
            "compression_level": attrs.int(default = default_compression_level),
        },
        cfg = package_cfg,
    )

_cas_dir, _cas_dir_anon = _new_package_rule(
    format = "cas_dir",
    is_dir = True,
    sudo = True,
)

_cpio, _cpio_anon = _new_package_rule(
    format = "cpio",
    sudo = True,
)

_cpio_gz = _new_compressed_package_rule(
    default_compression_level = 3,
    uncompressed = _cpio_anon,
    compressor = "gzip",
)

_cpio_zst = _new_compressed_package_rule(
    default_compression_level = 15,
    uncompressed = _cpio_anon,
    compressor = "zstd",
)

_rpm, _rpm_anon = _new_package_rule(
    rule_attrs = {
        "arch": attrs.enum(
            ["x86_64", "aarch64", "noarch"],
            default = arch_select(x86_64 = "x86_64", aarch64 = "aarch64"),
        ),
        "autoprov": attrs.bool(default = True),
        "autoreq": attrs.bool(default = True),
        "changelog": attrs.option(attrs.string(), default = None),
        "conflicts": attrs.list(attrs.string(), default = []),
        "description": attrs.option(attrs.string(), default = None),
        "epoch": attrs.int(default = 0),
        "extra_files": attrs.list(attrs.string(), default = []),
        "license": attrs.string(),
        "packager": attrs.option(attrs.string(), default = None),
        "post_install_script": attrs.option(attrs.string(), default = None),
        "provides": attrs.list(attrs.string(), default = []),
        "python_bytecompile": attrs.bool(default = True),
        "recommends": attrs.list(attrs.string(), default = []),
        "release": attrs.option(attrs.string(), default = None, doc = "If unset, defaults to current datetime YYYYMMDD"),
        "requires": attrs.list(attrs.string(), default = []),
        "requires_post": attrs.list(attrs.string(), default = []),
        "rpm_name": attrs.string(),
        "sign_digest_algo": attrs.option(attrs.string(), default = None),
        "sign_with_private_key": attrs.option(attrs.source(), default = None),
        "summary": attrs.option(attrs.string(), default = None),
        "supplements": attrs.list(attrs.string(), default = []),
        "version": attrs.option(attrs.string(), default = None, doc = "If unset, defaults to current datetime HHMMSS"),
    },
    format = "rpm",
    dot_meta = False,
    sudo = True,
    force_extension = "rpm",
)

_vfat, _vfat_anon = _new_package_rule(
    rule_attrs = {
        "fat_size": attrs.option(attrs.int(), default = None),
        "label": attrs.option(attrs.string(), default = None),
        "size_mb": attrs.int(),
    },
    format = "vfat",
    sudo = True,
    can_be_partition = True,
)

_squashfs, squashfs_anon = _new_package_rule(
    rule_attrs = {},
    format = "squashfs",
    can_be_partition = True,
    sudo = True,
)

_tar, _tar_anon = _new_package_rule(
    format = "tar",
    sudo = True,
)

_tar_gz = _new_compressed_package_rule(
    default_compression_level = 3,
    uncompressed = _tar_anon,
    compressor = "gzip",
)

_tar_zst = _new_compressed_package_rule(
    default_compression_level = 15,
    uncompressed = _tar_anon,
    compressor = "zstd",
)

_ext3, _ext3_anon = _new_package_rule(
    format = "ext3",
    rule_attrs = {
        "free_mb": attrs.int(
            default = 0,
            doc = "include at least this much free space in the image",
        ),
        "label": attrs.option(attrs.string(), default = None),
        "size_mb": attrs.option(
            attrs.int(),
            default = None,
            doc = "absolute size of the image",
        ),
    },
    can_be_partition = True,
    sudo = True,
)

_unprivileged_dir, _unprivileged_dir_anon = _new_package_rule(
    format = "unprivileged_dir",
    is_dir = True,
    sudo = True,
)

package = struct(
    btrfs = btrfs,
    cas_dir = package_macro(_cas_dir),
    cpio = package_macro(_cpio),
    cpio_gz = package_macro(_cpio_gz),
    cpio_zst = package_macro(_cpio_zst),
    ext3 = package_macro(_ext3),
    gpt = gpt,
    rpm = package_macro(_rpm),
    sendstream_v2 = sendstream_v2,
    squashfs = package_macro(_squashfs),
    tar = package_macro(_tar),
    tar_gz = package_macro(_tar_gz),
    tar_zst = package_macro(_tar_zst),
    unprivileged_dir = package_macro(_unprivileged_dir),
    vfat = package_macro(_vfat),
)
