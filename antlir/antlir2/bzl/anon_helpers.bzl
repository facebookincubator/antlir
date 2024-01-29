# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def _new_rule(*, impl, attrs, artifact_promise_mappings = None):
    r = anon_rule(
        impl = impl,
        attrs = attrs,
        artifact_promise_mappings = artifact_promise_mappings or {},
    )

    # Extract all attributes with a default value to easily propagate them up to
    # the outer rule that's going to instantiate this anon_target.
    #
    # This makes it easy to add default(_only) deps to anonymous rules without
    # having to explicitly remember to propagate it down from the outer rule(s)
    # - antlir2 makes heavy use of anonymous targets and there are some rules
    # that get instantiated anonymously from many different rules.
    outer_attr_prefix = "_anon_default_" + repr(impl).replace(".", "_") + "_"
    default_outer_attr_names = {
        outer_attr_prefix + inner: inner
        for inner, attr in attrs.items()
        if "default=" in repr(attr)
    }
    default_outer_attrs = {
        outer: native.attrs.default_only(attrs[inner])
        for outer, inner in default_outer_attr_names.items()
    }

    # @lint-ignore BUCKRESTRICTEDSYNTAX
    def _anon_target(ctx: AnalysisContext, *, **kwargs):
        for outer, inner in default_outer_attr_names.items():
            if not hasattr(ctx.attrs, outer):
                fail("rule is not using anon_helpers correctly: missing attr '{}'".format(outer))
            kwargs.setdefault(inner, getattr(ctx.attrs, outer))

        return ctx.actions.anon_target(r, kwargs)

    return struct(
        anon_target = _anon_target,
        default_outer_attrs = default_outer_attrs,
    ), r

anon_helpers = struct(
    new_rule = _new_rule,
)
