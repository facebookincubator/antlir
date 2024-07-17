# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/package:defs.bzl", "package")
load("//antlir/antlir2/package_managers/dnf/rules:rpm.bzl", "rpm")
load("//antlir/bzl:types.bzl", "types")

types.lint_noop()

def test_rpm(
        *,
        name: str,
        version: str,
        release: str,
        epoch: int = 0,
        arch: str = "noarch",
        license: str = "NONE",
        requires: list[str] = [],
        requires_post: list[str] = [],
        provides: list[str] = [],
        recommends: list[str] = [],
        features = [],
        parent_layer: str | None = None,
        post_install_script: str | None = None,
        changelog: str | None = None) -> str:
    target_name = name + "-" + version + "-" + release + "." + arch
    image.layer(
        name = target_name + "--layer",
        features = features,
        parent_layer = parent_layer,
        rootless = True,
        visibility = [":{}--package".format(target_name)],
    )
    package.rpm(
        name = target_name + "--package",
        layer = ":{}--layer".format(target_name),
        arch = arch,
        epoch = epoch,
        version = version,
        release = release,
        rpm_name = name,
        license = license,
        requires = requires,
        requires_post = requires_post,
        provides = provides,
        recommends = recommends,
        post_install_script = post_install_script,
        changelog = changelog,
        visibility = [":" + target_name],
    )
    rpm(
        name = target_name,
        arch = arch,
        epoch = epoch,
        release = release,
        rpm = ":{}--package".format(target_name),
        rpm_name = name,
        sha256 = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        version = version,
        visibility = [
            "//antlir/antlir2/features/rpm/tests/...",
        ],
    )
    return ":" + target_name
