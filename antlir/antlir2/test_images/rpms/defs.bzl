# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")
load("//antlir/bzl:build_defs.bzl", "buck_command_alias")

def test_foo_version_installed_script(version: str.type) -> str.type:
    name = "test-foo-installed-" + version
    if native.rule_exists(name):
        return ":" + name

    buck_command_alias(
        name = name,
        args = [version],
        exe = ":test-foo-version.sh",
    )
    return ":" + name

def test_foo_version_installed(name: str.type, layer: str.type, version: str.type):
    image_sh_test(
        name = name,
        layer = layer,
        test = test_foo_version_installed_script(version),
    )
