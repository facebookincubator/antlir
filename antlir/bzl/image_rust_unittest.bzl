# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":image_unittest_helpers.bzl", helpers = "image_unittest_helpers")
load(":oss_shim.bzl", "buck_sh_test", "get_visibility", "python_binary", "rust_unittest")

def image_rust_unittest(
        name,
        layer,
        boot = False,
        run_as_user = None,
        hostname = None,
        container_opts = None,
        visibility = None,
        **rust_unittest_kwargs):
    wrapper_props = helpers.nspawn_wrapper_properties(
        name = name,
        layer = layer,
        test_type = "rust",
        boot = boot,
        run_as_user = run_as_user,
        inner_test_kwargs = rust_unittest_kwargs,
        extra_outer_kwarg_names = [],
        visibility = [],
        hostname = hostname,
        container_opts = container_opts,
    )

    rust_unittest(
        name = helpers.hidden_test_name(name, "rust"),
        antlir_rule = "user-internal",
        labels = helpers.tags_to_hide_test(),
        **wrapper_props.inner_test_kwargs
    )

    wrapper_binary = name + "_test-wrapper"
    python_binary(
        name = wrapper_binary,
        main_module = "antlir.nspawn_in_subvol.run_test",
        deps = [wrapper_props.impl_python_library],
        # Ensures we can read resources in @mode/opt.  "xar" cannot work
        # because `root` cannot access the content of unprivileged XARs.
        par_style = "zip",
        antlir_rule = "user-internal",
        visibility = [],
    )

    buck_sh_test(
        name = name,
        env = wrapper_props.outer_test_kwargs.pop("env", {}),
        antlir_rule = "user-facing",
        test = ":" + wrapper_binary,
        type = "rust",
        visibility = get_visibility(visibility),
    )
