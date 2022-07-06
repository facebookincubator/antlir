# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "get_visibility")

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
        visibility = get_visibility(visibility),
        antlir_rule = "user-internal",
    )

def bzl_to_py(name, bzl_target, imports):
    """
    Convert .bzl file provided by target into an importable .py file
    """

    buck_genrule(
        name = name,
        cmd = """
set -eu
bzl="$(location {bzl_target})"
echo "{imports}" > $OUT
# small hack to keep line numbers the same as the original source,
# remove the first {lines_to_remove} lines which are supposed to be comments,
# fail if they aren't
if head -n {lines_to_remove} "$bzl" | grep -v '[[:space:]]*#'; then
    echo "First {lines_to_remove} lines of \"$bzl\" file aren't comments"
    exit 1
fi
tail -n +{lines_to_remove} "$bzl" >> $OUT
        """.format(imports = "\n".join(imports), bzl_target = bzl_target, lines_to_remove = len(imports) + 1),
    )
