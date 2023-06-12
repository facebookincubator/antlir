# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/testing:image_test.bzl?v2_only", antlir2_image_python_test = "image_python_test")
load(":antlir2_shim.bzl", "antlir2_shim")
load(":build_defs.bzl", "add_test_framework_label", "is_buck2", "python_unittest")
load(":flavor.shape.bzl", "flavor_t")
load(":image_unittest_helpers.bzl", helpers = "image_unittest_helpers")
load(":types.bzl", "types")

# This exists to hack around a complex FB-internal migration. *sigh*
# It should be removable when this is done:  https://fburl.com/nxc3u5mk
_TEMP_TP_TAG = "use-testpilot-adapter"

_OPTIONAL_STRUCT = types.optional(types.struct)

types.lint_noop(flavor_t, _OPTIONAL_STRUCT)

def image_python_unittest(
        name,
        layer,
        boot = False,
        run_as_user = None,
        visibility = None,
        par_style = None,
        hostname = None,
        container_opts = None,
        flavor = None,
        flavor_config_override: _OPTIONAL_STRUCT = None,
        antlir2 = None,
        **python_unittest_kwargs):
    visibility = visibility or []

    wrapper_props = helpers.nspawn_wrapper_properties(
        name = name,
        layer = layer,
        test_type = "pyunit",
        boot = boot,
        run_as_user = run_as_user,
        inner_test_kwargs = python_unittest_kwargs,
        # Future: there is probably a "generically correct" way of handling
        # `needed_coverage`, but we'll find it later.
        extra_outer_kwarg_names = ["needed_coverage"],
        visibility = visibility,
        hostname = hostname,
        container_opts = container_opts,
        flavor = flavor,
        flavor_config_override = flavor_config_override,
    )

    wrapper_props.outer_test_kwargs["tags"] = \
        add_test_framework_label(
            wrapper_props.outer_test_kwargs.pop("tags", []),
            "test-framework=7:antlir_image_test",
        ) + ["no_pyre"]

    # `par_style` only applies to the inner test that runs the actual user
    # code, because there is only one working choice for the outer test.
    # For the inner test:
    #   - Both `zip` and `fastzip` are OK, but the latter is the default
    #     since it should be more kind to `/tmp` `tmpfs` memory usage.
    #   - XAR fails to work for tests that run unprivileged (the default)
    #     My quick/failed attempt to fix this is in P61015086, but we'll
    #     probably be better off adding support for copying python trees
    #     directly into the image in preference to fixing XAR.
    if par_style == None:
        # People who need to access the filesystem will have to set "zip",
        # but that'll cost more RAM to run since nspawn `/tmp` is `tmpfs`.
        par_style = "fastzip"
    elif par_style == "xar":
        fail(
            "`image.python_unittest` does not support this due to XAR " +
            "limitations (see the in-code docs)",
            "par_style",
        )

    inner_tags = add_test_framework_label(
        helpers.tags_to_hide_test(),
        "test-framework=7:antlir_image_test",
    )

    # This is used by Buck2
    inner_tags = inner_tags + ["antlir_inner_test"]

    if _TEMP_TP_TAG in wrapper_props.outer_test_kwargs.get("tags", {}):
        inner_tags = inner_tags + [_TEMP_TP_TAG]

    python_unittest(
        name = helpers.hidden_test_name(name),
        tags = inner_tags,
        par_style = par_style,
        visibility = visibility,
        antlir_rule = "user-internal",
        supports_static_listing = False,
        **wrapper_props.inner_test_kwargs
    )

    # This outer "test" is not a test at all, but a wrapper that passes
    # arguments to the inner test binary.  It is a `python_unittest` since:
    #
    #  - That invokes the "pyunit" test runner, which is required to
    #    correctly interact with the inner test.
    #
    #  - It is **also** possible to use `sh_test` either with
    #    `type = "pyunit"` or with a tag of `custom-type-pyunit` to trigger
    #    the "pyunit" test runner.  However, Buck does not support the
    #    `needed_coverage` argument on `sh_test`, so this seemingly
    #    language-independent approach would break some important test
    #    features.
    #
    # Future: See Q18889 for an attempt to convince the Buck team to allow
    # `sh_test` to supply all the special testing arguments that other tests
    # use, like `needed_coverage`, `additional_coverage_targets`, and maybe
    # a few others.  This should not be a big deal, since Buck passes all
    # that data to the test runner as JSON, and lets it handle the details.
    # Then, we could plausibly have the same `sh_test` logic for all
    # languages.
    #
    # Future: It may be useful to also set `needed_coverage` on the inner
    # test, search for `_get_par_build_args` in the fbcode macros.
    python_unittest(
        name = name,
        # These dependencies must be on the user-visible "porcelain" target,
        # see the helper code for the explanation.
        resources = {
            target: "_dep_for_test_wrapper_{}".format(idx)
            for idx, target in enumerate(wrapper_props.porcelain_deps)
        },
        main_module = "antlir.nspawn_in_subvol.run_test",
        deps = [wrapper_props.impl_python_library],
        visibility = visibility,
        antlir_rule = "user-facing",  # This runs in customer TARGETS files
        supports_static_listing = False,
        **wrapper_props.outer_test_kwargs
    )

    if antlir2_shim.should_make_parallel_test(antlir2):
        if is_buck2():
            antlir2_image_python_test(
                name = name + ".antlir2",
                layer = layer + ".antlir2",
                boot = boot,
                run_as_user = run_as_user,
                boot_requires_units = ["dbus.socket"] if (boot and wrapper_props.container_opts.boot_await_dbus) else [],
                **python_unittest_kwargs
            )
        else:
            antlir2_shim.fake_buck1_test(name = name, test = "python")
