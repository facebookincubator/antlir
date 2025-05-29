# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "alias", "toolchain_alias")

def dep_distance_extender(
        *,
        name: str,
        actual: str,
        hops: int = 5,
        toolchain: bool = False,
        target_compatible_with = None,
        visibility: list[str] = []):
    """
    Extends the dependency distance of a target by adding n='hops' alias targets
    in between 'name' and 'actual'.

    Used to create artificial distance to discourage CI from thinking that the
    dependencies are connected enough to be worth testing.
    """
    alias_rule = alias if not toolchain else toolchain_alias
    for i in range(hops):
        i = i + 1
        top = i == hops
        bottom = i == 1
        alias_rule(
            name = name if top else "{}--hop{}".format(name, i),
            actual = actual if bottom else ":{}--hop{}".format(name, i - 1),
            target_compatible_with = target_compatible_with,
            visibility = visibility if top else [],
        )
