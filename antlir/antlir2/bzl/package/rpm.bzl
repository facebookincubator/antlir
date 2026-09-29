# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select", "os_select")
load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo", "LayerInfo")
load("//antlir/bzl:internal_external.bzl", "internal_external")
load(":attrs.bzl", "common_attrs", "default_attrs")
load(":cfg.bzl", "package_cfg")
load(":macro.bzl", "package_macro")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    layer = ctx.attrs.layer
    build_appliance = ctx.attrs.build_appliance

    output_name = ctx.attrs.out or ctx.label.name
    if not output_name.endswith(".rpm"):
        output_name += ".rpm"

    package = ctx.actions.declare_output(output_name, has_content_based_path = False)

    rpm_spec = {
        "arch": ctx.attrs.arch,
        "autoprov": ctx.attrs.autoprov,
        "autoreq": ctx.attrs.autoreq,
        "binary_payload": ctx.attrs.binary_payload,
        "build_appliance": build_appliance[BuildApplianceInfo].dir,
        "build_requires": ctx.attrs.build_requires,
        "changelog": ctx.attrs.changelog,
        "conflicts": ctx.attrs.conflicts,
        "description": ctx.attrs.description,
        "dirs": ctx.attrs.dirs,
        "disable_build_id_links": ctx.attrs.disable_build_id_links,
        "disable_ldconfig": ctx.attrs.disable_ldconfig,
        "disable_strip": ctx.attrs.disable_strip,
        "epoch": ctx.attrs.epoch,
        "extra_files": ctx.attrs.extra_files,
        "license": ctx.attrs.license,
        "os": ctx.attrs.os,
        "packager": ctx.attrs.packager,
        "post_install_script": ctx.attrs.post_install_script,
        "post_uninstall_script": ctx.attrs.post_uninstall_script,
        "pre_uninstall_script": ctx.attrs.pre_uninstall_script,
        "provides": ctx.attrs.provides,
        "python_bytecompile": ctx.attrs.python_bytecompile,
        "recommends": ctx.attrs.recommends,
        "release": ctx.attrs.release,
        "requires": ctx.attrs.requires,
        "requires_post": ctx.attrs.requires_post,
        "requires_post_uninstall": ctx.attrs.requires_post_uninstall,
        "requires_pre_uninstall": ctx.attrs.requires_pre_uninstall,
        "rpm_name": ctx.attrs.rpm_name,
        "sign_digest_algo": ctx.attrs.sign_digest_algo,
        "sign_with_private_key": ctx.attrs.sign_with_private_key,
        "summary": ctx.attrs.summary,
        "supplements": ctx.attrs.supplements,
        "transfiletriggerpostun_paths": ctx.attrs.transfiletriggerpostun_paths,
        "transfiletriggerpostun_script": ctx.attrs.transfiletriggerpostun_script,
        "version": ctx.attrs.version,
        "_strip": ctx.attrs._strip[RunInfo] if ctx.attrs._strip != None else None,
    }
    spec = ctx.actions.write_json(
        "spec.json",
        {"rpm": rpm_spec},
        with_inputs = True,
        has_content_based_path = False,
    )
    ctx.actions.run(
        cmd_args(
            ctx.attrs._antlir2_packager[RunInfo],
            cmd_args(spec, format = "--spec={}"),
            cmd_args(ctx.attrs._working_format, format = "--working-format={}"),
            cmd_args(
                {
                    "btrfs": layer[LayerInfo].contents.subvol_symlink,
                    "cad-stack": layer[LayerInfo].contents.cad_stack,
                }[ctx.attrs._working_format],
                format = "--layer={}",
            ),
            cmd_args(package.as_output(), format = "--out={}"),
            "--rootless" if ctx.attrs._rootless else cmd_args(),
        ),
        local_only = ctx.attrs._working_format == "btrfs",
        category = "antlir2_package",
        identifier = "rpm",
    )

    return [DefaultInfo(package)]

_rpm = rule(
    impl = _impl,
    attrs = default_attrs
    | common_attrs
    | {
        "arch": attrs.enum(
            ["x86_64", "aarch64", "noarch"],
            default = arch_select(x86_64 = "x86_64", aarch64 = "aarch64"),
        ),
        "autoprov": attrs.bool(default = True),
        "autoreq": attrs.bool(default = True),
        "binary_payload": attrs.option(attrs.string(), default = None),
        "build_requires": attrs.list(attrs.string(), default = []),
        "changelog": attrs.option(attrs.string(), default = None),
        "conflicts": attrs.list(attrs.string(), default = []),
        "description": attrs.option(attrs.string(), default = None),
        "dirs": attrs.list(
            attrs.string(),
            default = [],
            doc = "List of directories that will be explictly 'owned' by the rpm. Dirs must already exist in the package contents.",
        ),
        "disable_build_id_links": attrs.bool(default = False),
        "disable_ldconfig": attrs.bool(default = False),
        "disable_strip": attrs.bool(default = False),
        "epoch": attrs.int(default = 0),
        "extra_files": attrs.list(attrs.string(), default = []),
        "license": attrs.string(),
        "os": attrs.enum(
            ["linux", "darwin"],
            default = os_select(linux = "linux", macos = "darwin"),
        ),
        "packager": attrs.option(attrs.string(), default = None),
        "post_install_script": attrs.option(attrs.string(), default = None),
        "post_uninstall_script": attrs.option(attrs.string(), default = None),
        "pre_uninstall_script": attrs.option(attrs.string(), default = None),
        "provides": attrs.list(attrs.string(), default = []),
        "python_bytecompile": attrs.bool(default = True),
        "recommends": attrs.list(attrs.string(), default = []),
        "release": attrs.option(attrs.string(), default = None, doc = "If unset, defaults to current datetime YYYYMMDD"),
        "requires": attrs.list(attrs.string(), default = []),
        "requires_post": attrs.list(attrs.string(), default = []),
        "requires_post_uninstall": attrs.list(attrs.string(), default = []),
        "requires_pre_uninstall": attrs.list(attrs.string(), default = []),
        "rpm_name": attrs.string(),
        "sign_digest_algo": attrs.option(attrs.string(), default = None),
        "sign_with_private_key": attrs.option(attrs.source(), default = None),
        "summary": attrs.option(attrs.string(), default = None),
        "supplements": attrs.list(attrs.string(), default = []),
        "transfiletriggerpostun_paths": attrs.list(attrs.string(), default = []),
        "transfiletriggerpostun_script": attrs.option(attrs.string(), default = None),
        "version": attrs.option(attrs.string(), default = None, doc = "If unset, defaults to current datetime HHMMSS"),
        "_strip": internal_external(
            fb = attrs.default_only(attrs.exec_dep(default = "fbsource//third-party/binutils:strip")),
            oss = attrs.option(attrs.exec_dep(), default = None),
        ),
    },
    cfg = package_cfg,
)

rpm = package_macro(_rpm, always_rootless = True)
