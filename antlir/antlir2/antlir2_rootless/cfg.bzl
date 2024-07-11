# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

_refs = {
    "rooted": "antlir//antlir/antlir2/antlir2_rootless:rooted",
    "rootless": "antlir//antlir/antlir2/antlir2_rootless:rootless",
}

_attrs = {
    "rootless": attrs.option(attrs.bool(), default = None),
}

def _transition(*, refs, attrs, constraints):
    rootless = refs.rootless[ConstraintValueInfo]

    # If there is already a configuration, keep it
    if rootless.setting.label in constraints:
        return constraints
    elif attrs.rootless:
        # Otherwise set it to rootless if rootless=True otherwise default to
        # rooted
        constraints[rootless.setting.label] = rootless
    else:
        constraints[rootless.setting.label] = refs.rooted[ConstraintValueInfo]

    return constraints

_is_rootless_select = select({
    "DEFAULT": False,
    "antlir//antlir/antlir2/antlir2_rootless:rooted": False,
    "antlir//antlir/antlir2/antlir2_rootless:rootless": True,
})

def _transition_impl(platform, refs, attrs):
    constraints = platform.configuration.constraints
    constraints = _transition(refs = refs, attrs = attrs, constraints = constraints)
    return PlatformInfo(
        label = platform.label,
        configuration = ConfigurationInfo(
            constraints = constraints,
            values = platform.configuration.values,
        ),
    )

_rule_cfg = transition(
    impl = _transition_impl,
    attrs = _attrs.keys(),
    refs = _refs,
)

rootless_cfg = struct(
    refs = _refs,
    attrs = _attrs,
    transition = _transition,
    is_rootless_attr = attrs.default_only(attrs.bool(default = _is_rootless_select)),
    is_rootless_select = _is_rootless_select,
    rule_cfg = _rule_cfg,
)
