# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")
load("//antlir/bzl:types.bzl", "types")

types.lint_noop()

def test_deb(*, name: str, version: str, arch: str = "all", features = [], **kwargs) -> str:
    target_name = name + "_" + version + "_" + arch
    image.layer(
        name = target_name + "--layer",
        features = features,
        rootless = True,
        visibility = [":" + target_name],
    )
    package.deb(name = target_name, layer = ":{}--layer".format(target_name), arch = arch, deb_name = name, version = version, **kwargs)
    return ":" + target_name
