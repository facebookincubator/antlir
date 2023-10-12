# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/testing:image_test.bzl?v2_only", antlir2_image_rust_test = "image_rust_test")
load(":antlir2_shim.bzl", "antlir2_shim")
load(":build_defs.bzl", "buck_sh_test", "get_visibility", "is_buck2", "python_binary", "rust_unittest")
load(":image_unittest_helpers.bzl", helpers = "image_unittest_helpers")

def image_rust_unittest(
        name,
        layer,
        boot = False,
        run_as_user = None,
        hostname = None,
        container_opts = None,
        visibility = None,
        antlir2 = None,
        antlir2_requires_units = None,
        antlir2_after_units = None,
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
        tags = ["no_pyre"],
    )

    buck_sh_test(
        name = name,
        env = wrapper_props.outer_test_kwargs.pop("env", {}),
        antlir_rule = "user-facing",
        test = ":" + wrapper_binary,
        type = "rust",
        visibility = get_visibility(visibility),
        **wrapper_props.outer_test_kwargs
    )

    if antlir2_shim.should_shadow_test(antlir2):
        if is_buck2():
            antlir2_image_rust_test(
                name = name + ".antlir2",
                layer = layer + ".antlir2",
                boot = boot,
                run_as_user = run_as_user,
                boot_requires_units = ((
                    ["dbus.socket"] if (boot and wrapper_props.container_opts.boot_await_dbus) else []
                ) + (antlir2_requires_units or [])) if (boot and wrapper_props.container_opts.boot_await_dbus) else antlir2_requires_units,
                boot_after_units = antlir2_after_units,
                crate = rust_unittest_kwargs.pop("crate", name + "_unittest"),
                **rust_unittest_kwargs
            )
        else:
            antlir2_shim.fake_buck1_test(name = name)
