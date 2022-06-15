# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:add_stat_options.bzl", "add_stat_options")
load("//antlir/bzl:shape.bzl", "shape")
load(
    "//antlir/bzl/image_actions:ensure_subdirs_exist.shape.bzl",
    "ensure_subdirs_exist_t",
)
load(
    "//antlir/compiler/image/feature/buck2:rules.bzl",
    "maybe_add_feature_rule",
)

def _generate_shape(into_dir, subdirs_to_create, mode, user, group):
    dir_spec = {"into_dir": into_dir, "subdirs_to_create": subdirs_to_create}
    add_stat_options(dir_spec, mode, user, group)
    return shape.new(ensure_subdirs_exist_t, **dir_spec)

def _image_ensure_subdirs_exist(
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

def image_ensure_subdirs_exist(
        into_dir,
        subdirs_to_create,
        mode = None,
        user = None,
        group = None):
    """
    `image.ensure_subdirs_exist("/w/x", "y/z")` creates the directories `/w/x/y`
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
    return _image_ensure_subdirs_exist(
        into_dir = into_dir,
        subdirs_to_create = subdirs_to_create,
        mode = mode,
        user = user,
        group = group,
        name = "ensure_subdirs_exist",
    )

def image_ensure_dirs_exist(path, mode = None, user = None, group = None):
    """Equivalent to `image.ensure_subdirs_exist("/", path, ...)`."""
    return _image_ensure_subdirs_exist(
        into_dir = "/",
        subdirs_to_create = path,
        mode = mode,
        user = user,
        group = group,
        name = "ensure_dirs_exist",
    )

def _self_test():
    # Tests to make sure that _generate_shape returns proper shape
    shape_1 = shape.new(
        ensure_subdirs_exist_t,
        into_dir = "/foo/bar",
        subdirs_to_create = "baz",
        mode = "a+rwx",
        user_group = "root:1234",
    )
    shape_2 = shape.new(
        ensure_subdirs_exist_t,
        into_dir = "foo/bar/baz",
        subdirs_to_create = "foo",
        mode = 0o765,
        user_group = "1234:root",
    )

    test_shape_1 = _generate_shape(
        into_dir = "/foo/bar",
        subdirs_to_create = "baz",
        mode = "a+rwx",
        user = None,
        group = 1234,
    )
    test_shape_2 = _generate_shape(
        into_dir = "foo/bar/baz",
        subdirs_to_create = "foo",
        mode = 0o765,
        user = 1234,
        group = None,
    )

    if (shape_1 != test_shape_1 or
        shape_2 != test_shape_2):
        fail(
            "{function} failed test".format(
                function = "_generate_shape",
            ),
        )

_self_test()
