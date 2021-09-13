# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def _timezone(zone, timezone_dir = "/usr/share/zoneinfo"):
    """
    Build Antlir image features to support setting the timezone to the provided
    `zone`.

    In the unlikley event that the target `image.layer` this is used on has a
    non-standard timezone dir, override this via the `timezone_dir` param.
    """

    dest = "/etc/localtime"

    return [
        feature.remove(
            dest,
            must_exist = False,
        ),
        image.ensure_file_symlink(
            paths.join(timezone_dir, zone),
            dest,
        ),
    ]

time = struct(
    timezone = _timezone,
)
