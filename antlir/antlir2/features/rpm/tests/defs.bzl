# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:configured_alias.bzl", "antlir2_configured_alias")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")
load("//antlir/bzl:build_defs.bzl", "buck_command_alias")
load("//antlir/bzl:internal_external.bzl", "is_facebook")
load("//antlir/bzl:types.bzl", "types")

types.lint_noop()

expected_t = record(
    installed = field(list[str], default = []),
    userinstalled = field(list[str], default = []),
    installed_not_userinstalled = field(list[str], default = []),
    not_installed = field(list[str], default = []),
    modules_unknown = field(list[str], default = []),
    modules_installed = field(list[str], default = []),
)

def test_rpms(
        name: str,
        expected: expected_t,
        features: list[typing.Any] = [],
        parent_layer: str | None = None,
        dnf_additional_repos: list[str] = ["//antlir/antlir2/features/rpm/tests/repo:test-repo"],
        dnf_available_repos: str | None = None,
        dnf_versionlock: str | None = None,
        dnf_versionlock_extend: dict[str, str] | None = None,
        dnf_excluded_rpms: list[str] | None = None,
        labels: list[str] | None = None,
        also_eln_dnf5: bool = False,
        build_appliance: str | None = None):
    buck_command_alias(
        name = name + "--script",
        exe = "//antlir/antlir2/features/rpm/tests:test-installed-rpms",
        args = ["--expected", json.encode(expected), select({
            "//antlir/antlir2/os/package_manager:dnf": "--dnf-version=dnf4",
            "//antlir/antlir2/os/package_manager:dnf5": "--dnf-version=dnf5",
            "DEFAULT": "none",
        })],
    )
    image.layer(
        name = name + "--layer",
        parent_layer = parent_layer,
        features = features + [
            feature.remove(path = "/etc/dnf/dnf.conf", must_exist = False),
            feature.install(src = "antlir//antlir:empty", dst = "/etc/dnf/dnf.conf"),
        ],
        dnf_additional_repos = dnf_additional_repos,
        dnf_versionlock = dnf_versionlock,
        dnf_versionlock_extend = dnf_versionlock_extend,
        dnf_excluded_rpms = dnf_excluded_rpms,
        dnf_available_repos = dnf_available_repos,
        build_appliance = build_appliance,
    )
    image_sh_test(
        name = name,
        layer = ":{}--layer".format(name),
        test = ":{}--script".format(name),
        labels = labels,
    )
    if also_eln_dnf5 and is_facebook:
        antlir2_configured_alias(
            name = name + "-dnf5",
            actual = ":" + name,
            default_os = "eln",
        )

    return ":{}--layer".format(name)
