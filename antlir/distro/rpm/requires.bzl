# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/bzl:build_defs.bzl", "buck_genrule")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")

def rpm_requires(binary: str):
    name = "rpm-deps-for-" + binary.replace(":", "_")
    if not native.rule_exists(name):
        buck_genrule(
            name = name,
            out = "subjects.txt",
            bash = """
                $(exe antlir//antlir/distro/rpm:find-requires) \
                    $OUT \
                    $(location {binary}) \
            """.format(binary = binary),
        )
    return normalize_target(":" + name)

def install_with_rpm_requires(*, src: str, **kwargs):
    req = rpm_requires(src)
    return [
        feature.rpms_install(subjects_src = req),
        feature.install(src = src, **kwargs),
    ]

def _query_all_binary_requirements_impl(ctx: AnalysisContext) -> list[Provider]:
    requires = ctx.actions.declare_output("requires.txt")
    ctx.actions.run(
        cmd_args(
            ctx.attrs._find_requires[RunInfo],
            requires.as_output(),
            [binary[DefaultInfo].default_outputs[0] for binary in ctx.attrs.q],
        ),
        category = "find_requires",
    )
    return [DefaultInfo(requires)]

_query_all_binary_requirements_rule = rule(
    impl = _query_all_binary_requirements_impl,
    attrs = {
        "q": attrs.query(),
        "_find_requires": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/distro/rpm:find-requires")),
    },
)

_query_all_binary_requirements = rule_with_default_target_platform(_query_all_binary_requirements_rule)

def install_all_binary_rpm_requirements(layer: str):
    name = "rpm-requires-{}".format(layer.replace(":", "_"))
    _query_all_binary_requirements(
        name = name,
        q = "kind(cxx_binary, deps({}, 10000, target_deps()))".format(layer),
    )
    return feature.rpms_install(subjects_src = normalize_target(":" + name))
