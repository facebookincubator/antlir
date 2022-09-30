# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:oss_shim.bzl", "buck_genrule")

def dummy_rule(name, deps = None, visibility = None, **kwargs):
    """
    Helps make the buck1 dependency graph mirror buck2 dependency graph by
    allowing us to add rules in buck1 that have the same name and dependencies
    as the new rules in buck2.
    """
    if not native.rule_exists(name):
        buck_genrule(
            name = name,
            type = "dummy_rule",
            bash = """
            # {deps}
            touch $OUT
            """.format(
                deps = " ".join([
                    "$(location {})".format(t)
                    for t in sorted(deps if deps else [])
                ]),
            ),
            visibility = visibility,
            antlir_rule = "user-internal",
            **kwargs
        )

    return ":" + name
