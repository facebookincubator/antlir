# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable
# @oss-disable
# @oss-disable
load("@prelude//utils:selects.bzl", "selects")
load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:cfg.bzl", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/bzl:build_defs.bzl", "add_test_framework_label", "buck_sh_test", "cpp_unittest", "internal_external", "is_facebook", "python_unittest", "rust_unittest")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl:oss_shim.bzl", "special_tags") # @oss-enable

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
    boot_wants_units = _default_list(ctx.attrs.boot_wants_units, default = ["default.target"])

    mounts = {}
    for mount in ctx.attrs.layer[LayerInfo].mounts:
        if mount.layer:
            mounts[mount.layer.mountpoint] = mount.layer.subvol_symlink
        if mount.host:
            mounts[mount.host.mountpoint] = mount.host.src
    spec = ctx.actions.write_json(
        "spec.json",
        {
            "boot": {
                "after_units": boot_after_units,
                "requires_units": boot_requires_units,
                "wants_units": boot_wants_units,
            } if ctx.attrs.boot else None,
            "hostname": ctx.attrs.hostname,
            "layer": ctx.attrs.layer[LayerInfo].contents.subvol_symlink,
            "mounts": mounts,
            "pass_env": ctx.attrs.test[ExternalRunnerTestInfo].env.keys(),
            "rootless": ctx.attrs._rootless,
            "user": ctx.attrs.run_as_user,
        },
        with_inputs = True,
    )

    test_cmd = cmd_args(
        ctx.attrs.image_test[RunInfo],
        cmd_args(spec, format = "--spec={}"),
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
        cmd_args(
            "#!/bin/bash",
            cmd_args(
                test_cmd,
                delimiter = " \\\n  ",
            ),
            "\n",
        ),
        is_executable = True,
        allow_args = True,
    )

    env = dict(ctx.attrs.test[ExternalRunnerTestInfo].env)
    if ctx.attrs._static_list_wrapper:
        original = env.pop("STATIC_LIST_TESTS_BINARY", None)
        if original:
            env["STATIC_LIST_TESTS_BINARY"] = RunInfo(cmd_args(
                ctx.attrs._static_list_wrapper[RunInfo],
                cmd_args(original, format = "--wrap={}"),
            ))

    return [
        ExternalRunnerTestInfo(
            command = [test_cmd],
            type = ctx.attrs.test[ExternalRunnerTestInfo].test_type,
            labels = ctx.attrs.labels + inner_labels,
            contacts = ctx.attrs.test[ExternalRunnerTestInfo].contacts,
            env = env,
            run_from_project_root = True,
        ),
        RunInfo(test_cmd),
        DefaultInfo(
            script,
            sub_targets = {
                "inner_test": ctx.attrs.test.providers,
                "layer": ctx.attrs.layer.providers,
            },
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
        "boot_wants_units": attrs.option(
            attrs.list(
                attrs.string(),
            ),
            default = None,
            doc = "Add a Wants= requirement on these units to the test",
        ),
        "hostname": attrs.option(attrs.string(), default = None),
        "image_test": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/testing/image_test:image-test")),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
        "run_as_user": attrs.string(default = "root"),
        "test": attrs.dep(providers = [ExternalRunnerTestInfo]),
        "_rootless": rootless_cfg.is_rootless_attr,
        "_static_list_wrapper": attrs.option(attrs.exec_dep(), default = None),
    } | cfg_attrs(),
    doc = "Run a test inside an image layer",
    cfg = layer_cfg,
)

image_test = rule_with_default_target_platform(_image_test)

# Collection of helpers to create the inner test implicitly, and hide it from
# TestPilot

def _implicit_image_test(
        test_rule,
        name: str,
        layer: str,
        run_as_user: str | None = None,
        labels: list[str] | Select | None = None,
        boot: bool = False,
        boot_requires_units: [list[str], None] = None,
        boot_after_units: [list[str], None] = None,
        boot_wants_units: [list[str], None] = None,
        hostname: str | None = None,
        _add_outer_labels: list[str] = [],
        default_os: str | None = None,
        # @oss-disable
        rootless: bool | None = None,
        _static_list_wrapper: str | None = None,
        exec_compatible_with: list[str] | Select | None = None,
        target_compatible_with: list[str] | Select | None = None,
        visibility: list[str] | None = None,
        **kwargs):
    test_rule(
        name = name + "_image_test_inner",
        labels = add_test_framework_label(HIDE_TEST_LABELS, "test-framework=7:antlir_image_test") + [
            # never schedule any CI on this inner target
            # @oss-disable
        ],
        **kwargs
    )

    labels = selects.apply(
        labels or [],
        lambda labels: labels + _add_outer_labels,
    )

    # @oss-disable
        # @oss-disable

    if rootless == None:
        rootless = get_antlir2_rootless()

    if boot:
        image.layer(
            name = "{}--bootable-layer".format(name),
            parent_layer = layer,
            features = [
                systemd.install_unit(
                    "antlir//antlir/antlir2/testing/image_test:antlir2_image_test.service",
                    force = True,
                ),
            ],
            default_os = default_os,
            # @oss-disable
            rootless = rootless,
            labels = ["antlir2-implicit-layer=image_test_boot"],
        )
        layer = ":{}--bootable-layer".format(name)

        # TODO(T187078382): booted tests still must go through systemd-nspawn
        rootless = False

    if rootless == False:
        target_compatible_with = selects.apply(
            target_compatible_with or [],
            lambda tcw: tcw + ["antlir//antlir/antlir2/antlir2_rootless:rooted"],
        )
        labels = selects.apply(labels, lambda labels: labels + ["uses_sudo"])

    image_test(
        name = name,
        layer = layer,
        run_as_user = run_as_user,
        test = ":" + name + "_image_test_inner",
        labels = labels + [special_tags.enable_artifact_reporting],
        boot = boot,
        boot_requires_units = boot_requires_units,
        boot_after_units = boot_after_units,
        boot_wants_units = boot_wants_units,
        hostname = hostname,
        default_os = default_os,
        # @oss-disable
        rootless = rootless,
        _static_list_wrapper = _static_list_wrapper,
        exec_compatible_with = exec_compatible_with,
        target_compatible_with = target_compatible_with,
        visibility = visibility,
    )

image_cpp_test = partial(
    _implicit_image_test,
    cpp_unittest,
    _static_list_wrapper = "antlir//antlir/antlir2/testing/image_test:static-list-cpp",
    _add_outer_labels = ["tpx:optout-test-result-output-spec"] + internal_external(
        fb = [],
        # don't have working gtest in oss (yet)
        oss = ["disabled"],
    ),
)

image_rust_test = partial(_implicit_image_test, rust_unittest)
image_sh_test = partial(_implicit_image_test, buck_sh_test)

def image_python_test(
        name: str,
        layer: str,
        default_os: str | None = None,
        default_rou: str | None = None,
        **kwargs):
    test_layer = layer
    if not REPO_CFG.artifacts_require_repo and is_facebook:
        # In @mode/opt we need to install fb-xarexec
        test_layer = name + "--with-xarexec"
        image.layer(
            name = test_layer,
            parent_layer = layer,
            features = [
                feature.rpms_install(rpms = ["fb-xarexec"]),
            ],
            visibility = [":" + name],
            default_os = default_os,
            # @oss-disable
            labels = ["antlir2-implicit-layer=image_test_xarexec"],
        )
        test_layer = ":{}".format(test_layer)

    _implicit_image_test(
        test_rule = python_unittest,
        name = name,
        layer = test_layer,
        default_os = default_os,
        # @oss-disable
        _static_list_wrapper = "antlir//antlir/antlir2/testing/image_test:static-list-py",
        **kwargs
    )
