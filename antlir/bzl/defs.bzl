# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:oss_shim.bzl", "buck_genrule")

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
