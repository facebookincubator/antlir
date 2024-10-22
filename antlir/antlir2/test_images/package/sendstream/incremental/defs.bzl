# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/test_images/package:defs.bzl", "standard_features")

# install some rpms we need to run the genrule feature (antlir doesn't
# really allow first-class in-place mutations)
_needed_rpms = [
    "coreutils",
    "bash",
    "attr",
]

def child_layer(
        *,
        name: str,
        parent_layer: str):
    image.layer(
        name = name + "-mutate",
        parent_layer = parent_layer,
        features = [
            feature.ensure_dirs_exist(dirs = "/standard"),
            standard_features(prefix = "/incremental"),
            feature.remove(
                path = "/to-be-removed",
            ),
            feature.rpms_install(rpms = _needed_rpms),
            feature.genrule(
                bash = """
                echo World! >> /hello

                setfattr -n user.foo -v qux /i-will-get-new-xattrs
                setfattr -x user.bar /i-will-get-new-xattrs
                setfattr -n user.baz -v baz /i-will-get-new-xattrs

                ln -sf /goodbye /aloha
            """,
                user = "root",
            ),
        ],
        visibility = [":" + name],
    )
    image.layer(
        name = name,
        parent_layer = ":" + name + "-mutate",
        features = [
            feature.rpms_remove(rpms = _needed_rpms),
            feature.remove(path = "/usr"),
            feature.remove(path = "/var"),
        ],
    )
