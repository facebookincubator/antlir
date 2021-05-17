# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":oss_shim.bzl", "buck_genrule", "get_visibility")

def fake_macro_library(name, srcs, deps = None, visibility = None):
    """
    This rule does not build anything useful! Its only job is to inform
    `buck query`-powered dependency resolvers that `image_*` targets depend
    on the underlying macros.

    Without these rules, we would not automatically trigger the appropriate
    builds & tests on changes to the macro code, which would make it easy to
    accidentally break trunk.

    This should eventually become unnecessary, follow Q10141.

    Future: It'd be great to enforce that `deps` can only refer to rules of
    type `fake_macro_library`.  I'm not sure how to do that without writing
    a full-fledged converter, though.
    """
    if deps == None:
        deps = []
    buck_genrule(
        name = name,
        srcs = srcs,
        out = name,
        # The point of this command is to convince Buck that this rule
        # depends on its sources, and the transitive closure of its
        # dependencies.  The output is a recursive hash, so it should change
        # whenever any of the inputs change.
        bash = 'sha384sum $SRCS {dep_locations} > "$OUT"'.format(
            name = name,
            dep_locations = " ".join([
                "$(location {})".format(d)
                for d in sorted(deps)
            ]),
        ),
        type = "fake_macro_library",
        visibility = get_visibility(visibility, name),
        antlir_rule = "user-internal",
    )
