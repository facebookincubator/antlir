# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/bzl:stat.bzl", "stat")
load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn", "data_only_feature_rule")

def ensure_subdirs_exist(
        *,
        into_dir: str | Select,
        subdirs_to_create: str | Select,
        mode: int | str | Select = 0o755,
        user: str | Select = "root",
        group: str | Select = "root") -> list[ParseTimeFeature]:
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
            plugin = antlir2_dep("features:ensure_dir_exists"),
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
        dirs: str,
        mode: int | str = 0o755,
        user: str = "root",
        group: str = "root") -> list[ParseTimeFeature]:
    """Equivalent to `ensure_subdirs_exist("/", dirs, ...)`."""
    return ensure_subdirs_exist(
        into_dir = "/",
        subdirs_to_create = dirs,
        mode = mode,
        user = user,
        group = group,
    )

ensure_dir_exists_record = record(
    dir = str,
    mode = int,
    user = str,
    group = str,
)

# TODO: delete this when chef_solo is migrated to an anon rule
ensure_dir_exists_analyze = data_only_feature_analysis_fn(
    ensure_dir_exists_record,
    feature_type = "ensure_dir_exists",
)

ensure_dir_exists_rule = data_only_feature_rule(
    feature_type = "ensure_dir_exists",
    feature_attrs = {
        "dir": attrs.string(),
        "group": attrs.string(),
        "mode": attrs.int(),
        "user": attrs.string(),
    },
)
