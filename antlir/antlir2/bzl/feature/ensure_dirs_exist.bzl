# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:stat.bzl", "stat")
load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn")

_STR_OR_SELECTOR = types.or_selector(str.type)
_INT_OR_STR_OR_SELECTOR = types.or_selector([int.type, str.type])

types.lint_noop(_STR_OR_SELECTOR, _INT_OR_STR_OR_SELECTOR)

def ensure_subdirs_exist(
        *,
        into_dir: _STR_OR_SELECTOR,
        subdirs_to_create: _STR_OR_SELECTOR,
        mode: _INT_OR_STR_OR_SELECTOR = 0o755,
        user: _STR_OR_SELECTOR = "root",
        group: _STR_OR_SELECTOR = "root") -> [ParseTimeFeature.type]:
    """
    `ensure_subdirs_exist("/w/x", "y/z")` creates the directories `/w/x/y` and
    `/w/x/y/z` in the image, if they do not exist. `/w/x` must have already been
    created by another image feature. If any dirs to be created already exist in
    the image, their attributes will be checked to ensure they match the
    attributes provided here. If any do not match, the build will fail.

    The argument `mode` changes file mode bits of all directories in
    `subdirs_to_create`. It can be an integer fully specifying the bits or a
    symbolic string like `u+rx`. In the latter case, the changes are applied on
    top of mode 0.

    The arguments `user` and `group` change file owner and group of all
    directories in `subdirs_to_create`.
    """
    mode = stat.mode(mode) if mode else None
    features = []
    dir = into_dir
    for component in subdirs_to_create.split("/"):
        if not component:
            continue
        dir = paths.join(dir, component)
        features.append(ParseTimeFeature(
            feature_type = "ensure_dir_exists",
            kwargs = {
                "dir": dir,
                "group": group,
                "mode": mode,
                "user": user,
            },
        ))
    return features

def ensure_dirs_exist(
        *,
        dirs: str.type,
        mode: [int.type, str.type] = 0o755,
        user: str.type = "root",
        group: str.type = "root") -> [ParseTimeFeature.type]:
    """Equivalent to `ensure_subdirs_exist("/", dirs, ...)`."""
    return ensure_subdirs_exist(
        into_dir = "/",
        subdirs_to_create = dirs,
        mode = mode,
        user = user,
        group = group,
    )

ensure_dir_exists_record = record(
    dir = str.type,
    mode = int.type,
    user = str.type,
    group = str.type,
)

ensure_dir_exists_analyze = data_only_feature_analysis_fn(ensure_dir_exists_record)
