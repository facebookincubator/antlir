# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:stat.bzl", "stat")
load(
    "//antlir/bzl/image/feature:ensure_subdirs_exist.shape.bzl",
    "ensure_subdirs_exist_t",
)
load(
    "//antlir/compiler/image/feature/buck2:rules.bzl",
    "maybe_add_feature_rule",
)

def _generate_shape(into_dir, subdirs_to_create, mode, user, group):
    return ensure_subdirs_exist_t(
        into_dir = into_dir,
        subdirs_to_create = subdirs_to_create,
        mode = stat.mode(mode),
        user = user,
        group = group,
    )

def _feature_ensure_subdirs_exist(
        into_dir,
        subdirs_to_create,
        mode,
        user,
        group,
        name):
    return maybe_add_feature_rule(
        name = name,
        key = "ensure_subdirs_exist",
        include_in_target_name = {
            "into_dir": into_dir,
            "subdirs_to_create": subdirs_to_create,
        },
        feature_shape = _generate_shape(
            into_dir,
            subdirs_to_create,
            mode,
            user,
            group,
        ),
    )

def feature_ensure_subdirs_exist(
        into_dir,
        subdirs_to_create,
        mode = 0o755,
        user = "root",
        group = "root"):
    """
    `feature.ensure_subdirs_exist("/w/x", "y/z")` creates the directories `/w/x/y`
    and `/w/x/y/z` in the image, if they do not exist. `/w/x` must have already
    been created by another image feature. If any dirs to be created already
    exist in the image, their attributes will be checked to ensure they match
    the attributes provided here. If any do not match, the build will fail.

    The arguments `into_dir` and `subdirs_to_create` are mandatory; `mode`,
    `user`, and `group` are optional.

    The argument `mode` changes file mode bits of all directories in
    `subdirs_to_create`. It can be an integer fully specifying the bits or a
    symbolic string like `u+rx`. In the latter case, the changes are applied on
    top of mode 0.

    The arguments `user` and `group` change file owner and group of all
    directories in `subdirs_to_create`. `user` and `group` can be integers or
    symbolic strings. In the latter case, the passwd/group database from the
    host (not from the image) is used.
    """
    return _feature_ensure_subdirs_exist(
        into_dir = into_dir,
        subdirs_to_create = subdirs_to_create,
        mode = mode,
        user = user,
        group = group,
        name = "ensure_subdirs_exist",
    )

def feature_ensure_dirs_exist(path, mode = 0o755, user = "root", group = "root"):
    """Equivalent to `feature.ensure_subdirs_exist("/", path, ...)`."""
    return _feature_ensure_subdirs_exist(
        into_dir = "/",
        subdirs_to_create = path,
        mode = mode,
        user = user,
        group = group,
        name = "ensure_dirs_exist",
    )
