# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def _timezone(zone, timezone_dir = "/usr/share/zoneinfo", use_antlir2 = False):
    """
    Build Antlir image features to support setting the timezone to the provided
    `zone`.

    In the unlikley event that the target `image.layer` this is used on has a
    non-standard timezone dir, override this via the `timezone_dir` param.
    """

    dest = "/etc/localtime"

    if use_antlir2:
        return [
            antlir2_feature.remove(
                path = dest,
                must_exist = False,
            ),
            antlir2_feature.ensure_file_symlink(
                link = dest,
                target = paths.join(timezone_dir, zone),
            ),
        ]
    return [
        feature.remove(
            dest,
            must_exist = False,
        ),
        feature.ensure_file_symlink(
            paths.join(timezone_dir, zone),
            dest,
        ),
    ]

time = struct(
    timezone = _timezone,
)
