# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")
load("//antlir/bzl:build_defs.bzl", "buck_command_alias")
load("//antlir/bzl:types.bzl", "types")

types.lint_noop()

expected_t = record(
    installed = field(list[str], default = []),
    userinstalled = field(list[str], default = []),
    installed_not_userinstalled = field(list[str], default = []),
    not_installed = field(list[str], default = []),
    installed_module = field(list[str], default = []),
)

def test_rpms(
        name: str,
        expected: expected_t,
        features: list[types.antlir_feature] = [],
        parent_layer: str | None = None,
        flavor: str | None = None,
        dnf_additional_repos: list[str] = ["//antlir/antlir2/test_images/rpms/repo:test-repo"],
        dnf_versionlock: str | None = None):
    buck_command_alias(
        name = name + "--script",
        exe = "//antlir/antlir2/test_images/rpms:test-installed-rpms",
        args = [json.encode(expected)],
    )
    image.layer(
        name = name + "--layer",
        parent_layer = parent_layer,
        flavor = flavor,
        features = features + [
            feature.remove(path = "/etc/dnf/dnf.conf", must_exist = False),
            feature.install(src = "//antlir:empty", dst = "/etc/dnf/dnf.conf"),
        ],
        dnf_additional_repos = dnf_additional_repos,
        dnf_versionlock = dnf_versionlock,
    )
    image_sh_test(
        name = name,
        layer = ":{}--layer".format(name),
        test = ":{}--script".format(name),
    )
    return ":{}--layer".format(name)
