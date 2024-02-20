# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/features:feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")
load("//antlir/bzl:stat.bzl", "stat")

def ensure_subdirs_exist(
        *,
        into_dir: str | Select,
        subdirs_to_create: str | Select,
        mode: int | str | Select = 0o755,
        user: str | Select = "root",
        group: str | Select = "root") -> list[ParseTimeFeature]:
    """
    Ensure directories exist in the image (analogous to `mkdir -p`).

    Args:
        into_dir: Parent directory (must already exist)
        subdirs_to_create: Subdirectories to create under `into_dir`

            These subdirectories may already exist in the image. If so, they
            will be checked to ensure that the `mode` and `user:group` matches
            what is declared here.

        mode: set file mode bits of the newly-created directories
        user: set owning user of the newly-created directories
        group: set owning group of the newly-created directories
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
            plugin = antlir2_dep("//antlir/antlir2/features/ensure_dir_exists:ensure_dir_exists"),
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

ensure_dir_exists_rule = data_only_feature_rule(
    feature_type = "ensure_dir_exists",
    feature_attrs = {
        "build_phase": attrs.enum(BuildPhase.values(), default = "compile"),
        "dir": attrs.string(),
        "group": attrs.string(),
        "mode": attrs.int(),
        "user": attrs.string(),
    },
)
