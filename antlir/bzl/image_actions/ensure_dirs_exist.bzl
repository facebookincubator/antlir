# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:add_stat_options.bzl", "add_stat_options", "mode_t")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load("//antlir/bzl:target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")

ensure_subdirs_exist_t = shape.shape(
    into_dir = str,
    subdirs_to_create = str,
    mode = shape.field(mode_t, optional = True),
    user_group = shape.field(str, optional = True),
)

def image_ensure_dirs_exist(path, mode = None, user = None, group = None):
    """Equivalent to `image.ensure_subdirs_exist("/", path, ...)`."""
    return image_ensure_subdirs_exist(
        into_dir = "/",
        subdirs_to_create = path,
        mode = mode,
        user = user,
        group = group,
    )

def image_ensure_subdirs_exist(into_dir, subdirs_to_create, mode = None, user = None, group = None):
    """
  `image.ensure_subdirs_exist("/w/x", "y/z")` creates the directories `/w/x/y`
  and `/w/x/y/z` in the image, if they do not exist. `/w/x` must have already
  been created by another image feature. If any dirs to be created already exist
  in the image, their attributes will be checked to ensure they match the
  attributes provided here. If any do not match, the build will fail.

  The arguments `into_dir` and `subdirs_to_create` are mandatory; `mode`,
  `user`, and `group` are optional.

  The argument `mode` changes file mode bits of all directories in
  `subdirs_to_create`. It can be an integer fully specifying the bits or a
  symbolic string like `u+rx`. In the latter case, the changes are applied on
  top of mode 0.

  The arguments `user` and `group` change file owner and group of all
  directories in `subdirs_to_create`. `user` and `group` can be integers or
  symbolic strings. In the latter case, the passwd/group database from the host
  (not from the image) is used.
    """

    dir_spec = {"into_dir": into_dir, "subdirs_to_create": subdirs_to_create}
    add_stat_options(dir_spec, mode, user, group)
    ensure_subdirs_exist = shape.new(ensure_subdirs_exist_t, **dir_spec)
    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(ensure_subdirs_exist = [ensure_subdirs_exist]),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = [antlir_dep("bzl/image_actions:ensure_dirs_exist")],
    )
