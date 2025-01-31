# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable: 

def _unavailable_impl(ctx):
    fail("""
        This target is unavailable in the antlir system toolchain. It must be
        defined in antlir/distro/deps in order to be usable.

        If you're seeing this after running 'buck2 build $target', try `buck2 cquery 'somepath($target, {})'`
    """.format(ctx.label.raw_target()))

_unavailable = rule(
    impl = _unavailable_impl,
    attrs = {
        "labels": attrs.list(attrs.string(), default = []),
    },
)

def unavailable(name: str):
    """
    To have a working unconfigured buck2 target graph, we need to declare some
    libraries as stubs that are not expected to actually be usable but exist to
    keep the unconfigured graph happy.
    """
    _unavailable(
        name = name,
        labels = [
            # By definition, this thing won't build, so don't ever let CI try
            # @oss-disable: 
        ],
        visibility = ["PUBLIC"],
    )
