# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "buck_sh_test", "cpp_unittest", "python_unittest", "rust_unittest")
load("//antlir/staging/antlir2:antlir2_layer_info.bzl", "LayerInfo")

def _impl(ctx: "context") -> ["provider"]:
    test_cmd = cmd_args(
        ctx.attrs.antlir2[RunInfo],
        "test",
        cmd_args(ctx.attrs.layer[LayerInfo].subvol_symlink, format = "--layer={}"),
        cmd_args(ctx.attrs.run_as_user, format = "--user={}"),
        "--",
        ctx.attrs.test[ExternalRunnerTestInfo].command,
    )
    return [
        ExternalRunnerTestInfo(
            command = [test_cmd],
            type = ctx.attrs.test[ExternalRunnerTestInfo].test_type,
        ),
        RunInfo(test_cmd),
        DefaultInfo(),
    ]

image_test = rule(
    impl = _impl,
    attrs = {
        "antlir2": attrs.default_only(attrs.exec_dep(default = "//antlir/staging/antlir2/antlir2:antlir2")),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
        "run_as_user": attrs.string(default = "root"),
        "test": attrs.dep(providers = [ExternalRunnerTestInfo]),
    },
    doc = "Run a test inside an image layer",
)

# Collection of helpers to create the inner test implicitly, and hide it from
# TestPilot

def _implicit_image_test(
        test_rule,
        name: str.type,
        layer: str.type,
        run_as_user: [str.type, None] = None,
        labels: [[str.type], None] = None,
        **kwargs):
    test_rule(
        name = name + "_image_test_inner",
        antlir_rule = "user-internal",
        labels = ["disabled", "test_is_invisible_to_testpilot"],
        **kwargs
    )
    image_test(
        name = name,
        layer = layer,
        run_as_user = run_as_user,
        test = ":" + name + "_image_test_inner",
        labels = labels,
    )

image_cpp_test = partial(_implicit_image_test, cpp_unittest)
image_python_test = partial(_implicit_image_test, python_unittest)
image_rust_test = partial(_implicit_image_test, rust_unittest)
image_sh_test = partial(_implicit_image_test, buck_sh_test)
