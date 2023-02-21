# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "buck_genrule")

def hoist(
        name,
        layer,
        path,
        *,
        selector = None,
        force_dir = False,
        out = "out",
        executable = False,
        visibility = None,
        ignore_missing = False,
        **kwargs):
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

    # get files with selector:
    # output: "1.rpm 2"
    >>> hoist("target", layer = ":layer", path = "src_folder", selector = ["-maxdepth 1"], force_dir = True)

    # get a flat structure with files:
    # output: "other_file.rpm 1.rpm"
    >>> hoist("target", layer = ":layer", path = "src_folder", selector = ["-name '*.rpm'"], force_dir = True)
    """

    cp = "cp -r --reflink=auto --no-clobber \"$subvol/{}\" \"$OUT\"".format(path)
    if selector:
        cp = "find \"$subvol/{path}\" {selector} -print0".format(
            path = path,
            selector = " ".join(selector),
        ) + " | xargs -0 -I% cp -r --reflink=auto --no-clobber \"%\" \"$OUT\""

    if ignore_missing:
        cp = "({}) || true".format(cp)

    if force_dir:
        out = "."

    buck_genrule(
        name = name,
        out = out,
        bash = '''
            binary_path=( $(exe //antlir:find-built-subvol) )
            layer_loc="$(location {layer})"
            subvol=\\$( "${{binary_path[@]}}" "$layer_loc" )

            {cp}
        '''.format(
            layer = layer,
            cp = cp,
        ),
        visibility = visibility,
        executable = executable,
        **kwargs
    )
