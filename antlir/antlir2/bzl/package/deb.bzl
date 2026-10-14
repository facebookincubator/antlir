# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:types.bzl", "BuildApplianceInfo", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "attrs_selected_by_cfg")
load(":attrs.bzl", "common_attrs", "default_attrs")
load(":cfg.bzl", "package_cfg")
load(":macro.bzl", "package_macro")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    layer = ctx.attrs.layer

    output_name = ctx.attrs.out or ctx.label.name
    if not output_name.endswith(".deb"):
        output_name += ".deb"

    package = ctx.actions.declare_output(output_name, has_content_based_path = False)

    deb_spec = {
        "arch": ctx.attrs.arch,
        "breaks": ctx.attrs.breaks,
        "build_appliance": ctx.attrs.build_appliance[BuildApplianceInfo].dir,
        "conflicts": ctx.attrs.conflicts,
        "deb_name": ctx.attrs.deb_name,
        "depends": ctx.attrs.depends,
        "description": ctx.attrs.description,
        "maintainer": ctx.attrs.maintainer,
        "postinst": ctx.attrs.postinst,
        "postrm": ctx.attrs.postrm,
        "pre_depends": ctx.attrs.pre_depends,
        "preinst": ctx.attrs.preinst,
        "prerm": ctx.attrs.prerm,
        "priority": ctx.attrs.priority,
        "provides": ctx.attrs.provides,
        "recommends": ctx.attrs.recommends,
        "replaces": ctx.attrs.replaces,
        "section": ctx.attrs.section,
        "suggests": ctx.attrs.suggests,
        "summary": ctx.attrs.summary,
        "version": ctx.attrs.version,
    }
    spec = ctx.actions.write_json(
        "spec.json",
        {"deb": deb_spec},
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
        identifier = "deb",
    )

    return [DefaultInfo(package)]

_relations = attrs.list(attrs.string(), default = [])

_deb = rule(
    impl = _impl,
    attrs = default_attrs
    | common_attrs
    | {
        "arch": attrs.enum(
            ["amd64", "arm64", "all"],
            default = arch_select(x86_64 = "amd64", aarch64 = "arm64"),
        ),
        "breaks": _relations,
        # dpkg-deb comes from the image OS build appliance (not the
        # OS-independent packager appliance), so default_os must be a
        # Debian-family OS
        "build_appliance": attrs_selected_by_cfg["build_appliance"],
        "conflicts": _relations,
        "deb_name": attrs.string(),
        "depends": _relations,
        "description": attrs.option(attrs.string(), default = None),
        "maintainer": attrs.option(attrs.string(), default = None),
        "postinst": attrs.option(attrs.string(), default = None),
        "postrm": attrs.option(attrs.string(), default = None),
        "pre_depends": _relations,
        "preinst": attrs.option(attrs.string(), default = None),
        "prerm": attrs.option(attrs.string(), default = None),
        "priority": attrs.option(attrs.string(), default = None),
        "provides": _relations,
        "recommends": _relations,
        "replaces": _relations,
        "section": attrs.option(attrs.string(), default = None),
        "suggests": _relations,
        "summary": attrs.option(attrs.string(), default = None),
        "version": attrs.string(),
    },
    cfg = package_cfg,
)

deb = package_macro(_deb, always_rootless = True)
