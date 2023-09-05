# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable
# @oss-enable antlir2_cell = "antlir2"

def antlir2_dep(label):
    """
    Get a normalized target referring to a dependency under the root antlir2
    directory. This helper should be used when adding deps on antlir2 from macro
    layers.

    This should not be used for dependencies declared in TARGETS files.
    """

    if "//" in label or label.startswith("/"):
        fail(
            "antlir_dep should be expressed as a label relative to the " +
            "root antlir2 directory, e.g. instead of " +
            "`$cell//antlir/antlir2/foo:bar` the dep should be expressed " +
            "as `foo:bar`.",
        )

    if label.startswith(":"):
        return "{}//antlir/antlir2{}".format(antlir2_cell, label)
    return "{}//antlir/antlir2/{}".format(antlir2_cell, label)
