# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable
# @oss-disable
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/bzl:build_defs.bzl", "add_test_framework_label", "buck_sh_test", "cpp_unittest", "python_unittest", "rust_unittest")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:systemd.bzl", "systemd")

HIDE_TEST_LABELS = ["disabled", "test_is_invisible_to_testpilot"]

def _default_list(maybe_value: list[str] | None, default: list[str]) -> list[str]:
    if maybe_value == None:
        return default
    return maybe_value

def _impl(ctx: AnalysisContext) -> list[Provider]:
    if not ctx.attrs.boot and (ctx.attrs.boot_requires_units or ctx.attrs.boot_after_units):
        fail("boot=False cannot be combined with boot_{requires,after}_units")

    boot_requires_units = _default_list(ctx.attrs.boot_requires_units, default = ["sysinit.target"])
    boot_after_units = _default_list(ctx.attrs.boot_after_units, default = ["sysinit.target", "basic.target"])

    mounts = ctx.actions.write_json("mounts.json", ctx.attrs.layer[LayerInfo].mounts)

    test_cmd = cmd_args(
        ctx.attrs.image_test[RunInfo],
        cmd_args(ctx.attrs.layer[LayerInfo].subvol_symlink, format = "--layer={}"),
        cmd_args(ctx.attrs.run_as_user, format = "--user={}"),
        cmd_args(ctx.attrs.hostname, format = "--hostname={}") if ctx.attrs.hostname else cmd_args(),
        cmd_args("--boot") if ctx.attrs.boot else cmd_args(),
        cmd_args(boot_requires_units, format = "--requires-unit={}") if ctx.attrs.boot else cmd_args(),
        cmd_args(boot_after_units, format = "--after-unit={}") if ctx.attrs.boot else cmd_args(),
        cmd_args(["{}={}".format(k, v) for k, v in ctx.attrs.test[ExternalRunnerTestInfo].env.items()], format = "--setenv={}"),
        cmd_args(mounts, format = "--mounts={}"),
        ctx.attrs.test[ExternalRunnerTestInfo].test_type,
        ctx.attrs.test[ExternalRunnerTestInfo].command,
    )

    # Copy the labels from the inner test since there is tons of behavior
    # controlled by labels and we don't want to have to duplicate logic that
    # other people are already writing in the standard *_unittest macros.
    # This wrapper should be as invisible as possible.
    inner_labels = list(ctx.attrs.test[ExternalRunnerTestInfo].labels)
    for label in HIDE_TEST_LABELS:
        inner_labels.remove(label)

    script, _ = ctx.actions.write(
        "test.sh",
        cmd_args("#!/bin/bash", cmd_args(test_cmd, delimiter = " \\\n  "), "\n"),
        is_executable = True,
        allow_args = True,
    )
    return [
        ExternalRunnerTestInfo(
            command = [test_cmd],
            type = ctx.attrs.test[ExternalRunnerTestInfo].test_type,
            labels = ctx.attrs.labels + inner_labels,
            contacts = ctx.attrs.test[ExternalRunnerTestInfo].contacts,
            env = ctx.attrs.test[ExternalRunnerTestInfo].env,
            run_from_project_root = True,
        ),
        RunInfo(test_cmd),
        DefaultInfo(
            script,
            sub_targets = {"layer": ctx.attrs.layer.providers},
        ),
    ]

_image_test = rule(
    impl = _impl,
    attrs = {
        "boot": attrs.bool(
            default = False,
            doc = "boot the container with /init as pid1 before running the test",
        ),
        "boot_after_units": attrs.option(
            attrs.list(
                attrs.string(),
            ),
            default = None,
            doc = "Add an After= requirement on these units to the test",
        ),
        "boot_requires_units": attrs.option(
            attrs.list(
                attrs.string(),
            ),
            default = None,
            doc = "Add a Requires= and After= requirement on these units to the test",
        ),
        "hostname": attrs.option(attrs.string(), default = None),
        "image_test": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/testing/image_test:image-test")),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
        "run_as_user": attrs.string(default = "root"),
        "test": attrs.dep(providers = [ExternalRunnerTestInfo]),
    },
    doc = "Run a test inside an image layer",
)

image_test = rule_with_default_target_platform(_image_test)

# Collection of helpers to create the inner test implicitly, and hide it from
# TestPilot

def _implicit_image_test(
        test_rule,
        name: str,
        layer: str,
        run_as_user: str | None = None,
        labels: list[str] | None = None,
        boot: bool = False,
        boot_requires_units: [list[str], None] = None,
        boot_after_units: [list[str], None] = None,
        hostname: str | None = None,
        _add_outer_labels: list[str] = [],
        **kwargs):
    test_rule(
        name = name + "_image_test_inner",
        antlir_rule = "user-internal",
        labels = add_test_framework_label(HIDE_TEST_LABELS, "test-framework=7:antlir_image_test"),
        **kwargs
    )
    labels = list(labels) if labels else []
    labels.extend(_add_outer_labels)

    # @oss-disable
        # @oss-disable

    if boot:
        image.layer(
            name = "{}--bootable-layer".format(name),
            parent_layer = layer,
            features = [
                systemd.install_unit(
                    "//antlir/antlir2/testing/image_test:antlir2_image_test.service",
                    force = True,
                ),
            ],
        )
        layer = ":{}--bootable-layer".format(name)

    image_test(
        name = name,
        layer = layer,
        run_as_user = run_as_user,
        test = ":" + name + "_image_test_inner",
        labels = labels + [special_tags.enable_artifact_reporting],
        boot = boot,
        boot_requires_units = boot_requires_units,
        boot_after_units = boot_after_units,
        hostname = hostname,
    )

image_cpp_test = partial(
    _implicit_image_test,
    cpp_unittest,
    _add_outer_labels = ["tpx:optout-test-result-output-spec"],
)

image_rust_test = partial(_implicit_image_test, rust_unittest)
image_sh_test = partial(_implicit_image_test, buck_sh_test)

def image_python_test(name: str, layer: str, **kwargs):
    test_layer = layer
    if not REPO_CFG.artifacts_require_repo:
        # In @mode/opt we need to install fb-xarexec
        test_layer = name + "--with-xarexec"
        image.layer(
            name = test_layer,
            parent_layer = layer,
            features = [
                feature.rpms_install(rpms = ["fb-xarexec"]),
            ],
            visibility = [":" + name],
        )
        test_layer = ":{}".format(test_layer)

    _implicit_image_test(
        test_rule = python_unittest,
        name = name,
        layer = test_layer,
        supports_static_listing = False,
        **kwargs
    )
