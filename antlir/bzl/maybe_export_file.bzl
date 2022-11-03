# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
`maybe_export_file()` helps implement syntax sugar so that users of image
items like `install_data` can write `install_data("repo/path", ...)` instead
of the more verbose:

    export_file("file_in_repo")
    ... install_data(":file_in_repo", ...) ...

The implementation of `install_data` (and of other image items) invokes this
helper to accept:
  - a target path (must contain a `:`) OR
  - a path to a repo-relative file or directory (must NOT contain a `:`).

For the corner case of a a repo path that contains a colon, an explicit
`export_file` must still be used.

When generating an `export_file()` under the hood, we use a sigil prefix of
`_IMAGE_EXPORT_FILE__` for the Buck target name, in order to avoid possible
conflicts with targets defined by the user.

In the future, it would be possible to expose a helper function to let users
refer to these export targets (e.g.  `image.exported_file`), but it is
probably better if they just type `export_file("their/file")` instead.
"""

load("@bazel_skylib//lib:types.bzl", "types")
load(":build_defs.bzl", "export_file")

def maybe_export_file(source):
    if source == None or not types.is_string(source) or ":" in source:
        return source

    # `source` may contain slashes, and that's fine because Buck target
    # names are allowed to contain slashes.
    buck_target_name = "_IMAGE_EXPORT_FILE__" + source
    if native.rule_exists(buck_target_name):
        return ":" + buck_target_name

    export_file(
        name = buck_target_name,
        src = source,
        visibility = ["//visibility:private"],
        antlir_rule = "user-internal",
    )
    return ":" + buck_target_name
