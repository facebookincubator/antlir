# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:cfg.bzl", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package")
load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")

def _rpm_names_test_impl(ctx: AnalysisContext) -> list[Provider]:
    src = ctx.attrs.src
    if src and ctx.attrs.names:
        fail("'src' and 'names' are mutually exclusive")
    if not src:
        if ctx.attrs.names == None:
            fail("'names' is required if 'src' is omitted")
        src = ctx.actions.write("names.txt", "\n".join(ctx.attrs.names))
    script = ctx.actions.write(
        "test.sh",
        cmd_args(
            "#!/bin/bash",
            "set -e",
            cmd_args(
                ctx.attrs.image_rpms_test[RunInfo],
                "names",
                src,
                cmd_args(ctx.attrs.layer[LayerInfo].facts_db, format = "--facts-db={}"),
                cmd_args("--not-installed") if ctx.attrs.not_installed else cmd_args(),
                cmd_args("$@"),
                delimiter = " ",
            ),
            delimiter = "\n",
        ),
        is_executable = True,
        with_inputs = True,
    )
    return [
        DefaultInfo(),
        RunInfo(cmd_args(script)),
        ExternalRunnerTestInfo(
            type = "simple",
            command = [script],
            default_executor = CommandExecutorConfig(
                local_enabled = True,
                # it's easier to say remote_enabled=False than provide the
                # correct RE configuration that would be required if it were
                # True
                remote_enabled = False,
            ),
        ),
    ]

_rpm_names_test = rule(
    impl = _rpm_names_test_impl,
    attrs = {
        "image_rpms_test": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/antlir2/testing/image_rpms_test:image-rpms-test")),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
        "names": attrs.option(attrs.list(attrs.string()), default = None),
        "not_installed": attrs.bool(default = False),
        "src": attrs.option(attrs.source(), default = None),
    } | cfg_attrs(),
    cfg = layer_cfg,
)

_rpm_names_test_macro = rule_with_default_target_platform(_rpm_names_test)

def image_test_rpm_names(
        *,
        default_os: str | None = None,
        rootless: bool | None = None,
        **kwargs):
    rootless = rootless if rootless != None else get_antlir2_rootless()
    labels = kwargs.pop("labels", [])
    if not rootless:
        labels.append("uses_sudo")
    _rpm_names_test_macro(
        default_os = default_os or get_default_os_for_package(),
        rootless = rootless,
        labels = labels,
        # Test execution platform is not *usually* where tests run, but since
        # `image_diff_test` is `local_only=True`, use this to force exec_deps to
        # resolve to the host platform where the test is actually going to
        # execute
        exec_compatible_with = ["prelude//platforms:may_run_local"],
        **kwargs
    )

def _rpm_integrity_test_impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        DefaultInfo(),
        RunInfo(cmd_args(
            ctx.attrs.image_rpms_test[RunInfo],
            "integrity",
            cmd_args(ctx.attrs.ignored_files, format = "--ignored-file={}"),
            cmd_args(ctx.attrs.ignored_rpms, format = "--ignored-rpm={}"),
            "--layer=/layer",
        )),
    ]

_rpm_integrity_test = rule(
    impl = _rpm_integrity_test_impl,
    attrs = {
        "ignored_files": attrs.list(attrs.string(doc = "path that is allowed to fail integrity test"), default = []),
        "ignored_rpms": attrs.list(attrs.string(doc = "name of rpm that is ignored for integrity checks"), default = []),
        "image_rpms_test": attrs.default_only(attrs.dep(default = "antlir//antlir/antlir2/testing/image_rpms_test:image-rpms-test")),
    },
)

_rpm_integrity_test_macro = rule_with_default_target_platform(_rpm_integrity_test)

def image_test_rpm_integrity(
        name: str,
        layer: str,
        ignored_files: list[str] | Select = [],
        ignored_rpms: list[str] | Select = [],
        default_os: str | None = None,
        rootless: bool | None = None,
        **kwargs):
    """
    Verify the integrity of all installed RPMs to ensure that any changes done
    by an image will not be undone by any runtime rpm installation.
    """
    rootless = rootless if rootless != None else get_antlir2_rootless()

    _rpm_integrity_test_macro(
        name = name + "--script",
        ignored_files = ignored_files,
        ignored_rpms = ignored_rpms,
    )
    image.layer(
        name = name + "--layer",
        force_flavor = layer + "[flavor]",
        features = [
            feature.rpms_install(rpms = ["rpm"]),
            feature.layer_mount(
                source = layer,
                mountpoint = "/layer",
            ),
        ],
        rootless = rootless,
    )
    image_sh_test(
        name = name,
        layer = ":{}--layer".format(name),
        test = ":{}--script".format(name),
        default_os = default_os or get_default_os_for_package(),
        rootless = rootless,
        **kwargs
    )
