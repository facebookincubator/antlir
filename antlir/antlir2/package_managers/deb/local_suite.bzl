# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load(":suite.bzl", "DebSuiteInfo")

def _local_suite_impl(ctx: AnalysisContext) -> list[Provider]:
    distribution = ctx.attrs.distribution or ctx.label.name

    pool = {}
    for deb in ctx.attrs.debs:
        filename = "pool/main/" + deb.basename
        if filename in pool:
            fail("duplicate deb filename '{}'".format(deb.basename))
        pool[filename] = deb
    debs_dir = ctx.actions.symlinked_dir("debs", pool, has_content_based_path = False)

    archive_dir = ctx.actions.declare_output("archive", dir = True, has_content_based_path = False)
    ctx.actions.run(
        cmd_args(
            ctx.attrs._index[RunInfo],
            cmd_args(archive_dir.as_output(), format = "--out={}"),
            cmd_args(distribution, format = "--distribution={}"),
            [cmd_args(arch, format = "--arch={}") for arch in ctx.attrs.architectures],
            [cmd_args("--deb=" + filename + "=", deb, delimiter = "") for filename, deb in pool.items()],
        ),
        category = "deb_local_suite_index",
    )

    return [
        DefaultInfo(archive_dir, sub_targets = {"debs": [DefaultInfo(debs_dir)]}),
        DebSuiteInfo(
            archive_dir = archive_dir,
            archive_url = "",
            base_variant = ctx.attrs.base_variant,
            components = [],
            debs_dir = debs_dir,
            distribution = distribution,
            suite_baseurl = "",
        ),
    ]

_local_suite = rule(
    impl = _local_suite_impl,
    attrs = {
        "architectures": attrs.list(
            attrs.string(),
            default = select({
                "ovr_config//cpu:arm64": ["arm64"],
                "ovr_config//cpu:x86_64": ["amd64"],
            }),
        ),
        "base_variant": attrs.enum(["none", "minbase", "standard"], default = "none"),
        "debs": attrs.list(attrs.source()),
        "distribution": attrs.option(attrs.string(), default = None),
        "_index": attrs.default_only(
            attrs.exec_dep(
                providers = [RunInfo],
                default = "//antlir/antlir2/package_managers/deb:local-suite-index",
            )
        ),
    },
)

# An apt suite served from locally-built .deb files (e.g. from package.deb),
# usable as the `suite` of feature.apt_install
local_suite = rule_with_default_target_platform(_local_suite)
