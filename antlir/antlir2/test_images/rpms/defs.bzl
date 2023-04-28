# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")
load("//antlir/bzl:build_defs.bzl", "buck_command_alias")
load("//antlir/bzl:types.bzl", "types")

types.lint_noop()

expected_t = record(
    installed = field([str.type], default = []),
    not_installed = field([str.type], default = []),
)

def test_rpms(
        name: str.type,
        expected: expected_t.type,
        features: [types.antlir_feature],
        parent_layer: [str.type, None] = None,
        flavor: [str.type, None] = None,
        dnf_versionlock: [str.type, None] = None):
    buck_command_alias(
        name = name + "--script",
        exe = ":test-installed-rpms",
        args = [json.encode(expected)],
    )
    image.layer(
        name = name + "--layer",
        parent_layer = parent_layer,
        flavor = flavor,
        features = features,
        dnf_available_repos = ":test-repo-set",
        dnf_versionlock = dnf_versionlock,
    )
    image_sh_test(
        name = name,
        layer = ":{}--layer".format(name),
        test = ":{}--script".format(name),
    )
    return ":{}--layer".format(name)
