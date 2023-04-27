# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")
load("//antlir/bzl:build_defs.bzl", "buck_command_alias")

expected_t = record(
    installed = field([str.type], default = []),
    not_installed = field([str.type], default = []),
)

def test_rpms(name: str.type, layer: str.type, expected: expected_t.type):
    buck_command_alias(
        name = name + "--script",
        exe = ":test-installed-rpms",
        args = [json.encode(expected)],
    )
    image_sh_test(
        name = name,
        layer = layer,
        test = ":{}--script".format(name),
    )
