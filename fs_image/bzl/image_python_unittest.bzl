load(":oss_shim.bzl", "get_visibility", "python_unittest")
load(":image_unittest_helpers.bzl", helpers = "image_unittest_helpers")

# This exists to hack around a complex FB-internal migration. *sigh*
# It should be removable when this is done:  https://fburl.com/nxc3u5mk
_TEMP_TP_TAG = "use-testpilot-adapter"

def image_python_unittest(
        name,
        layer,
        boot = False,
        run_as_user = "nobody",
        visibility = None,
        par_style = None,
        hostname = None,
        serve_rpm_snapshots = (),
        **python_unittest_kwargs):
    visibility = get_visibility(visibility, name)

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
        caller_fake_library = "//fs_image/bzl:image_python_unittest",
        visibility = visibility,
        hostname = hostname,
        serve_rpm_snapshots = serve_rpm_snapshots,
    )

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

    python_unittest(
        name = helpers.hidden_test_name(name),
        tags = helpers.tags_to_hide_test() + (
            [] if _TEMP_TP_TAG not in wrapper_props.outer_test_kwargs.get(
                "tags",
                {},
            ) else [_TEMP_TP_TAG]
        ),
        par_style = par_style,
        visibility = visibility,
        fs_image_internal_rule = True,
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
        main_module = "fs_image.nspawn_in_subvol.run_test",
        deps = [wrapper_props.impl_python_library],
        # Ensures we can read resources in @mode/opt.  "xar" cannot work
        # because `root` cannot access the content of unprivileged XARs.
        par_style = "zip",
        visibility = visibility,
        **wrapper_props.outer_test_kwargs
    )
