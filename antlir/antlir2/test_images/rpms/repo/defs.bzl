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
        name: str.type,
        version: str.type,
        release: str.type,
        arch: str.type = "noarch",
        license: str.type = "NONE",
        requires: [str.type] = [],
        recommends: [str.type] = [],
        features: [types.antlir_feature] = [],
        parent_layer: [str.type, None] = None) -> str.type:
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
