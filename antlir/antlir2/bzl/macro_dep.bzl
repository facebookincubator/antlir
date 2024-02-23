# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable
antlir2_cell = "antlir2" # @oss-enable

def antlir2_dep(label):
    """
    Get a normalized target referring to a dependency under the root antlir2
    directory. This helper should be used when adding deps on antlir2 from
    Starlark macros.

    This should not be used for dependencies declared in TARGETS files.
    """

    prefix = "//antlir/"

    # Technically passing the fbcode// cell will work, but will break OSS so
    # let's fail here. In internal-only code that can safely use the full cell,
    # antlir2_dep() is unnecessary.
    if prefix.startswith("fbcode//antlir/"):
        fail("label '{}' should not contain fbcode cell when used with antlir2_dep".format(label))

    if not label.startswith(prefix):
        fail("label '{}' should start with //antlir/ so that VSCode go-to-definition works".format(label))
    label = label.removeprefix(prefix)
    return "".join([antlir2_cell, prefix, label])
