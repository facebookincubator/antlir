# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")

_refs = {
    "rooted": antlir2_dep("//antlir/antlir2/antlir2_rootless:rooted"),
    "rootless": antlir2_dep("//antlir/antlir2/antlir2_rootless:rootless"),
}

_attrs = {
    "rootless": attrs.option(attrs.bool(), default = None),
}

def _transition(*, refs, attrs, constraints):
    rootless = refs.rootless[ConstraintValueInfo]
    if attrs.rootless != None:
        if attrs.rootless:
            constraints[rootless.setting.label] = rootless
        else:
            constraints[rootless.setting.label] = refs.rooted[ConstraintValueInfo]
    elif rootless.setting.label not in constraints:
        default_is_rootless = (
            native.read_config("antlir2", "rootless", False) or
            # OnDemand image builds have been completely broken forever, so
            # attempting to turn it on is fine even while many antlir2 features
            # still require sudo/root
            native.read_config("sandcastle", "is_ondemand_machine", False)
        )

        if default_is_rootless:
            constraints[rootless.setting.label] = rootless
        else:
            # Adding the 'rooted' constraint to the configuration is not strictly
            # necessary, but does make it easier to debug when it shows up in `buck2
            # audit configurations`
            constraints[rootless.setting.label] = refs.rooted[ConstraintValueInfo]

    return constraints

_is_rootless_select = select({
    antlir2_dep("//antlir/antlir2/antlir2_rootless:rootless"): True,
    antlir2_dep("//antlir/antlir2/antlir2_rootless:rooted"): False,
    "DEFAULT": False,
})

rootless_cfg = struct(
    refs = _refs,
    attrs = _attrs,
    transition = _transition,
    is_rootless_attr = attrs.default_only(attrs.bool(default = _is_rootless_select)),
    is_rootless_select = _is_rootless_select,
)
