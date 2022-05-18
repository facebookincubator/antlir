# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":image_unittest_helpers.bzl", helpers = "image_unittest_helpers")
load(":oss_shim.bzl", "buck_sh_test", "cpp_unittest", "python_binary")

def image_cpp_unittest(
        name,
        layer,
        boot = False,
        run_as_user = None,
        visibility = None,
        hostname = None,
        container_opts = None,
        **cpp_unittest_kwargs):
    visibility = visibility or []

    wrapper_props = helpers.nspawn_wrapper_properties(
        name = name,
        layer = layer,
        test_type = "gtest",
        boot = boot,
        run_as_user = run_as_user,
        inner_test_kwargs = cpp_unittest_kwargs,
        extra_outer_kwarg_names = [],
        caller_fake_library = "//antlir/bzl:image_cpp_unittest",
        visibility = visibility,
        hostname = hostname,
        container_opts = container_opts,
    )

    cpp_unittest(
        name = helpers.hidden_test_name(name),
        tags = helpers.tags_to_hide_test(),
        visibility = visibility,
        antlir_rule = "user-internal",
        **wrapper_props.inner_test_kwargs
    )

    wrapper_binary = name + "__test-wrapper"
    python_binary(
        name = wrapper_binary,
        main_module = "antlir.nspawn_in_subvol.run_test",
        deps = [wrapper_props.impl_python_library],
        # Ensures we can read resources in @mode/opt.  "xar" cannot work
        # because `root` cannot access the content of unprivileged XARs.
        par_style = "zip",
        visibility = visibility,
        antlir_rule = "user-internal",
    )

    env = wrapper_props.outer_test_kwargs.pop("env", {})
    env.update({
        # These dependencies must be on the user-visible "porcelain"
        # target, see the helper code for the explanation.
        "_dep_for_test_wrapper_{}".format(idx): "$(location {})".format(
            target,
        )
        for idx, target in enumerate(wrapper_props.porcelain_deps + [
            # Without this extra dependency, Buck will fetch the
            # `cpp_unittest` from cache without also fetching
            # `wrapper_binary`.  However, `exec_nspawn_wrapper.c` needs
            # `wrapper_binary` to be present in the local `buck-out`.
            ":" + wrapper_binary,
        ])
    })

    # This is a `buck_sh_test` so that we don't have to wrap the wrapper
    # with another binary.  This works just fine for the limited uses we
    # have of `image_cpp_unittest`.
    buck_sh_test(
        name = name,
        test = ":" + wrapper_binary,
        antlir_rule = "user-facing",
        type = "gtest",
        env = env,
        visibility = visibility,
        **wrapper_props.outer_test_kwargs
    )
