# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("@fbcode_macros//build_defs:fully_qualified_test_name_rollout.bzl", "NAMING_ROLLOUT_LABEL", "fully_qualified_test_name_rollout")
# @oss-disable[end= ]: load("@fbsource//tools/build_defs:testpilot_defs.bzl", "special_tags")
# @oss-disable[end= ]: load("@fbsource//tools/target_determinator/macros:ci.bzl", "ci")
load("@prelude//utils:selects.bzl", "selects")
load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:binaries_require_repo.bzl", "binaries_require_repo")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:cfg.bzl", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/bzl:build_defs.bzl", "add_test_framework_label", "buck_sh_test", "cpp_unittest", "python_unittest", "rust_unittest")
load("//antlir/bzl:oss_shim.bzl", "special_tags") # @oss-enable

load("//antlir/bzl:internal_external.bzl", "internal_external", "is_facebook")

HIDE_TEST_LABELS = [special_tags.disabled, special_tags.test_is_invisible_to_testpilot]

def env_from_wrapped_test(wrapped_test):
    env = dict(wrapped_test[ExternalRunnerTestInfo].env)

    # Fix LLVM coverage for wrapped tests
    if "LLVM_COV" in env:
        if "LLVM_COVERAGE_ADDITIONAL_OBJECT_PATHS" in env:
            additional_object_paths = [env["LLVM_COVERAGE_ADDITIONAL_OBJECT_PATHS"]]
        else:
            additional_object_paths = []
        env["LLVM_COVERAGE_ADDITIONAL_OBJECT_PATHS"] = cmd_args(wrapped_test[DefaultInfo].default_outputs + additional_object_paths, delimiter = ";")
    return env

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
            "mount_platform": ctx.attrs.mount_platform,
            "mounts": mounts,
            "pass_env": ctx.attrs.test[ExternalRunnerTestInfo].env.keys(),
            "rootless": ctx.attrs._rootless,
            "user": ctx.attrs.run_as_user,
        },
        with_inputs = True,
    )

    env_args = [
        cmd_args("export ", k, "=", cmd_args(v, quote = "shell"), delimiter = "")
        for k, v in ctx.attrs.test[ExternalRunnerTestInfo].env.items()
    ]

    test_cmd = cmd_args(
        ctx.attrs.image_test[RunInfo],
        "spawn",
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

    test_script, _ = ctx.actions.write(
        "test.sh",
        cmd_args(
            "#!/bin/bash",
            cmd_args(*env_args),
            cmd_args(ctx.label.project_root, format = "cd {}"),
            cmd_args(
                "exec",
                test_cmd,
                '"$@"',
                delimiter = " \\\n  ",
            ),
            "\n",
        ),
        absolute = True,
        is_executable = True,
        allow_args = True,
        with_inputs = True,
    )

    container_script, _ = ctx.actions.write(
        "container.sh",
        cmd_args(
            "#!/bin/bash",
            cmd_args(*env_args),
            cmd_args(ctx.label.project_root, format = "cd {}"),
            cmd_args(
                "exec",
                ctx.attrs.image_test[RunInfo],
                "container",
                cmd_args(spec, format = "--spec={}"),
                ctx.attrs.test[ExternalRunnerTestInfo].test_type,
                ctx.attrs.test[ExternalRunnerTestInfo].command,
                '"$@"',
                delimiter = " \\\n  ",
            ),
            "\n",
        ),
        absolute = True,
        allow_args = True,
        is_executable = True,
        with_inputs = True,
    )

    env = env_from_wrapped_test(ctx.attrs.test)
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
            default_executor = CommandExecutorConfig(
                local_enabled = True,
                # Image test requires local subvolume and cannot be run on RE
                remote_enabled = False,
            ),
        ),
        RunInfo(test_script),
        DefaultInfo(
            test_script,
            sub_targets = {
                "container": [
                    RunInfo(container_script),
                    DefaultInfo(container_script),
                ],
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
        "mount_platform": attrs.bool(
            default = True,
            doc = "Mount runtime platform (aka /usr/local/fbcode) from the host",
        ),
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
        layer: str | Select,
        run_as_user: str | Select | None = None,
        labels: list[str] | Select | None = None,
        boot: bool = False,
        boot_requires_units: [list[str], None] = None,
        boot_after_units: [list[str], None] = None,
        boot_wants_units: [list[str], None] = None,
        hostname: str | None = None,
        _add_outer_labels: list[str] = [],
        default_os: str | None = None,
        # @oss-disable[end= ]: default_rou: str | None = None,
        systemd: str | None = None,
        mount_platform: bool | None = None,
        rootless: bool | None = None,
        _static_list_wrapper: str | None = None,
        exec_compatible_with: list[str] | Select | None = None,
        target_compatible_with: list[str] | Select | None = None,
        default_target_platform: str | None = None,
        visibility: list[str] | None = None,
        **kwargs):
    test_rule(
        name = name + "_image_test_inner",
        labels = add_test_framework_label(HIDE_TEST_LABELS, "test-framework=7:antlir_image_test") + [
            # never schedule any CI on this inner target
            # @oss-disable[end= ]: ci.skip_target(),
        ],
        **kwargs
    )

    labels = selects.apply(
        labels or [],
        lambda labels: labels + _add_outer_labels,
    )

    # @oss-disable[end= ]: if fully_qualified_test_name_rollout.use_fully_qualified_name():
        # @oss-disable[end= ]: labels = labels + [NAMING_ROLLOUT_LABEL]

    if rootless == None:
        rootless = get_antlir2_rootless()

    if boot:
        image.layer(
            name = "{}--bootable-layer".format(name),
            parent_layer = layer,
            features = [
                "antlir//antlir/antlir2/testing/image_test:features",
            ],
            rootless = rootless,
            # setting implicit_layer reason means that any flags that normally
            # control configuration don't actually do anything (default_os,
            # systemd, default_rou, etc)
            implicit_layer_reason = "image_test_boot",
            # this implicit layer will never be the thing to install systemd, so
            # explicitly leave it unconfigured and let the parent_layer enforce
            # its own systemd configuration (if any)
            systemd = "unconfigured",
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

    if not exec_compatible_with:
        # Test execution platform is not *usually* where tests run, but since
        # `image_diff_test` is `local_only=True`, use this to force exec_deps to
        # resolve to the host platform where the test is actually going to
        # execute
        exec_compatible_with = ["prelude//platforms:may_run_local"]

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
        # @oss-disable[end= ]: default_rou = default_rou,
        systemd = systemd or "unconfigured",
        mount_platform = mount_platform,
        rootless = rootless,
        _static_list_wrapper = _static_list_wrapper,
        exec_compatible_with = exec_compatible_with,
        target_compatible_with = target_compatible_with,
        default_target_platform = default_target_platform,
        visibility = visibility,
    )

image_cpp_test = partial(
    _implicit_image_test,
    cpp_unittest,
    _static_list_wrapper = "antlir//antlir/antlir2/testing/image_test:static-list-cpp",
    _add_outer_labels = ["tpx:optout-test-result-output-spec", "tpx:supports_coverage"] + internal_external(
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
        systemd: str | None = None,
        target_compatible_with: list[str] | Select | None = None,
        with_xarexec: bool = True,
        **kwargs):
    if is_facebook and with_xarexec:
        with_xarexec = name + "--with-xarexec"
        image.layer(
            name = with_xarexec,
            parent_layer = layer,
            features = [
                feature.rpms_install(rpms = ["fb-xarexec"]),
            ],
            visibility = [":" + name],
            # setting implicit_layer reason means that any flags that normally
            # control configuration don't actually do anything (default_os,
            # systemd, default_rou, etc)
            implicit_layer_reason = "image_test_xarexec",
            # this implicit layer will never be the thing to install systemd, so
            # explicitly leave it unconfigured and let the parent_layer enforce
            # its own systemd configuration (if any)
            systemd = "unconfigured",
            target_compatible_with = target_compatible_with,
        )

        # In opt modes, we need to use a parent_layer that has fb-xarexec
        # installed, otherwise the interpreter shebang will be unavailable to
        # run any installed XARs
        test_layer = selects.apply(
            binaries_require_repo.select_value,
            lambda binaries_require_repo: (":" + with_xarexec) if not binaries_require_repo else layer,
        )
    else:
        test_layer = layer

    _implicit_image_test(
        test_rule = python_unittest,
        name = name,
        layer = test_layer,
        default_os = default_os,
        # @oss-disable[end= ]: default_rou = default_rou,
        systemd = systemd,
        target_compatible_with = target_compatible_with,
        _static_list_wrapper = "antlir//antlir/antlir2/testing/image_test:static-list-py",
        **kwargs
    )
