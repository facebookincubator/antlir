# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@prelude//:rules.bzl", "config_setting", "constraint_value")

OsVersionInfo = provider(fields = [
    "constraint",
    "family",
    "package_manager",
])

def _os_version_rule_impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        DefaultInfo(),
        ctx.attrs.config_setting[ConfigurationInfo],
        OsVersionInfo(
            constraint = ctx.attrs.constraint,
            family = ctx.attrs.family,
            package_manager = ctx.attrs.package_manager,
        ),
    ]

_os_version_rule = rule(
    impl = _os_version_rule_impl,
    attrs = {
        "config_setting": attrs.dep(providers = [ConfigurationInfo]),
        "constraint": attrs.dep(providers = [ConstraintValueInfo]),
        "family": attrs.dep(providers = [ConstraintValueInfo]),
        "package_manager": attrs.dep(providers = [ConstraintValueInfo]),
    },
)

def os_version(
        name: str,
        family: str,
        package_manager: str):
    constraint_value(
        name = name + ".constraint",
        constraint_setting = "//antlir/antlir2/os:os",
        visibility = [":" + name],
    )
    config_setting(
        name = name + ".config",
        constraint_values = [
            ":{}.constraint".format(name),
            family,
            package_manager,
        ],
        visibility = ["PUBLIC"],
    )
    _os_version_rule(
        name = name,
        family = family,
        constraint = ":{}.constraint".format(name),
        config_setting = ":{}.config".format(name),
        package_manager = package_manager,
    )
