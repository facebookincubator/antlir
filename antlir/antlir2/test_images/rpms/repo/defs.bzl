# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")
load("//antlir/bzl:types.bzl", "types")
load("//antlir/rpm/dnf2buck:rpm.bzl", "rpm")

types.lint_noop()

def test_rpm(
        name: str,
        version: str,
        release: str,
        arch: str = "noarch",
        license: str = "NONE",
        requires: list[str] = [],
        recommends: list[str] = [],
        features = [],
        parent_layer: str | None = None,
        post_install_script: str | None = None) -> str:
    target_name = name + "-" + version + "-" + release + "." + arch
    image.layer(
        name = target_name + "--layer",
        features = features,
        flavor = "//antlir/antlir2/test_images:test-image-flavor",
        parent_layer = parent_layer,
        visibility = [":{}--package".format(target_name)],
    )
    package.rpm(
        name = target_name + "--package",
        layer = ":{}--layer".format(target_name),
        arch = arch,
        version = version,
        release = release,
        rpm_name = name,
        license = license,
        requires = requires,
        recommends = recommends,
        post_install_script = post_install_script,
        visibility = [":" + target_name],
    )
    rpm(
        name = target_name,
        arch = arch,
        epoch = 0,
        release = release,
        rpm = ":{}--package".format(target_name),
        rpm_name = name,
        sha256 = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        version = version,
        visibility = [
            "//antlir/antlir2/test_images/rpms/...",
        ],
    )
    return ":" + target_name
