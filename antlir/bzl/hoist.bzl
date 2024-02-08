# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:hoist.bzl?v2_only", antlir2_hoist = "hoist")
load("//antlir/bzl:build_defs.bzl", "buck_genrule")

def hoist(
        name,
        layer,
        path,
        *,
        force_dir = False,
        executable = False,
        selector = None,
        out = None,
        visibility = None):
    """
    Creates a rule to lift an artifact out of the image it was built in.

    By default, does a recursive cp. If selector is present, then it specifies
    "find" params and the results are copied to out.

    force_dir says that the output will have multiple items and to flatten them (mostly
    used along with the selector since that usually specifies multiple items)

    Usage:
    # input: "./src_file.txt ./other_file.rpm ./src_folder/1.rpm ./src_folder/2"

    # get a single file:
    # output: "file.txt"
    >>> hoist("target", layer = ":layer", path = "src_file.txt")

    # get a single file to folder output:
    # output: "out/file.txt"
    >>> hoist("target", layer = ":layer", path = "src_file.txt", force_dir = True)

    # get a single folder:
    # output: "folder/1.rpm folder/2"
    >>> hoist("target", layer = ":layer", path = "src_folder")
    """
    antlir2_hoist_name = name
    if selector:
        antlir2_hoist_name = name + "--before-selector"
        buck_genrule(
            name = name,
            out = out,
            cmd = "find $(location :{hoisted})/ {selector} -print0".format(
                hoisted = antlir2_hoist_name,
                selector = " ".join(selector),
            ) + " | xargs -0 -I% cp -r --reflink=auto --no-clobber \"%\" \"$OUT\"",
            antlir_rule = "user-facing",
            visibility = visibility,
        )

    antlir2_hoist(
        name = antlir2_hoist_name,
        dir = force_dir,
        executable = executable,
        layer = layer,
        path = path,
        out = out,
        visibility = visibility,
    )
