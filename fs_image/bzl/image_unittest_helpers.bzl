load("@bazel_skylib//lib:shell.bzl", "shell")
load("//fs_image/bzl/image_actions:install.bzl", "image_install_buck_runnable")
load(":image_layer.bzl", "image_layer")
load(":oss_shim.bzl", "buck_genrule", "python_library")
load(":rpm_repo_snapshot.bzl", "snapshot_install_dir")

def _hidden_test_name(name):
    # This is the test binary that is supposed to run inside the image.  The
    # "IGNORE-ME" prefix serves to inform users who come across this target
    # that this is not the test binary they are looking for.  It's a prefix
    # to avoid people stumbling across it via tab-completion.
    return "IGNORE-ME-layer-test--" + name

def _tags_to_hide_test():
    # These tags (aka labels) are a defense-in-depth attempt to make the
    # un-wrapped test never get executed by the test runner.
    return [
        # In `.buckconfig`, we have a line that asks Buck not to report
        # this test to the test runner if it's only being pulled in as a
        # transitive dependency:
        #
        #   [test]
        #     excluded_labels = exclude_test_if_transitive_dep
        #
        # This means that with `buck test //path:name`, the test runner
        # would never see IGNORE-ME tests.
        "exclude_test_if_transitive_dep",
        # Buck will still report the test to the test runner if the
        # user runs `buck test path/...`, which is a common pattern.
        # This tag tells the FB test runner NOT to run this test, nor to
        # show it as OMITTED.
        "test_is_invisible_to_testpilot",
        # For peace of mind, add classic test-runner tags that are
        # mutually incompatible, and would essentially always cause the
        # test to be marked OMITTED even if the prior two tags were
        # somehow ignored.
        "disabled",
        "local_only",
        "extended",
        "slow",
    ]

def _nspawn_wrapper_properties(
        name,
        layer,
        test_type,  # Has to be supported by `run_test.py`
        boot,
        run_as_user,
        inner_test_kwargs,
        extra_outer_kwarg_names,
        caller_fake_library,
        visibility,
        hostname,
        serve_rpm_snapshots):
    # Fail early, so the user doesn't have to wait for the test to build.
    # Future: we could potentially relax this since some odd applications
    # might want to just talk to the repo-server, and not install RPMs.
    if serve_rpm_snapshots and run_as_user != "root":
        fail(
            '"{}" needs `run_as_user = "root"` to install RPMs'.format(name),
            "serve_rpm_snapshots",
        )

    # These args must be on the outer wrapper test, regardless of language.
    outer_kwarg_names = ["tags", "env"]
    outer_kwarg_names.extend(extra_outer_kwarg_names)

    real_inner_test_kwargs = inner_test_kwargs.copy()
    outer_test_kwargs = {}
    for kwarg in outer_kwarg_names:
        if kwarg in real_inner_test_kwargs:
            outer_test_kwargs[kwarg] = real_inner_test_kwargs.pop(kwarg)

    # This target name gets a suffix to keep it discoverable via tab-completion
    test_layer = name + "--test-layer"

    # Make a test-specific image containing the test binary.
    binary_path = "/layer-test-binary.par"
    inner_test_target = ":" + _hidden_test_name(name)
    image_layer(
        name = test_layer,
        parent_layer = layer,
        features = [image_install_buck_runnable(inner_test_target, binary_path)],
        visibility = visibility,
    )

    # Generate a `.py` file that sets some of the key container options.
    #
    # NB: It would have been possible to use `env` to pass the arguments and
    # the location of the test layer to the driver binary.  However, this
    # would prevent one from running the test binary directly, bypassing
    # Buck.  Since Buck CLI is slow, this would be a significant drop in
    # usability, so we use this library trick.
    test_spec_py = "layer-test-spec-py-" + name
    buck_genrule(
        name = test_spec_py,
        out = "unused_name.py",
        bash = 'echo {} > "$OUT"'.format(shell.quote(("""\
import os
TEST_TYPE={test_type_repr}
def nspawn_in_subvol_args():
    return [
        '--user', {user_repr},
        *[
            '--setenv={{}}={{}}'.format(k, os.environ.get(k, ''))
                for k in {pass_through_env_repr}
        ],
        *[{maybe_boot}],
        *[{maybe_hostname}],
        *[
            '--serve-rpm-snapshot={{}}'.format(s)
                for s in {serve_rpm_snapshots_repr}
        ],
        '--', {binary_path_repr},
    ]
""").format(
            test_type_repr = repr(test_type),
            user_repr = repr(run_as_user),
            pass_through_env_repr = repr(outer_test_kwargs.get("env", [])),
            maybe_boot = "'--boot'" if boot else "",
            maybe_hostname = "'--hostname={hostname}'".format(hostname = hostname) if hostname else "",
            serve_rpm_snapshots_repr = repr([
                snapshot_install_dir(s)
                for s in serve_rpm_snapshots
            ]),
            binary_path_repr = repr(binary_path),
        ))),
        visibility = visibility,
        fs_image_internal_rule = True,
    )

    # To execute the wrapped test, the caller must make this library's
    # `fs_image.nspawn_in_subvol.run_test` the `main_module` of a
    # `python_binary`, and arrange for the binary to be run as if it were of
    # the inner test type.
    #
    # IMPORTANT: If you add more dependencies to THIS LIBRARY, or to any of
    # the AUXILIARY TARGETS above, you must add their externally observable
    # parts to `porcelain_deps` below, or CI test triggering will break.
    wrapper_impl_library = "layer-test-wrapper-library-" + name
    python_library(
        name = wrapper_impl_library,
        visibility = visibility,

        # This library puts the following files under
        # `fs_image/nspawn_in_subvol` in the source archive:
        #  - `run_test.py` with the business logic
        #  - `test_layer` from above
        #  - `test_spec_py` from above
        # This makes it easy for `nspawn_test_in_subvol()` to find its data.
        base_module = "fs_image.nspawn_in_subvol",
        deps = ["//fs_image/nspawn_in_subvol:run-test-library"],
        resources = {":" + test_layer: "nspawn-in-test-subvol-layer"},
        srcs = {":" + test_spec_py: "__image_python_unittest_spec__.py"},
        fs_image_internal_rule = True,
    )

    return struct(
        inner_test_kwargs = real_inner_test_kwargs,
        outer_test_kwargs = outer_test_kwargs,
        impl_python_library = ":" + wrapper_impl_library,
        # Users of `image.*_unittest` only see the outer "porcelain" target,
        # which internally wraps a hidden test binary.  Test code changes
        # directly affect the inner binary, while users naturally want the
        # **porcelain** target to be triggered by our CI determinators.
        #
        # In theory, the "porcelain" target already has a dependency on
        # `impl_python_library`, which -- several dependency hops later --
        # depends on the user-created test code.
        #
        # In practice, two things can go wrong, which are mitigated by this
        # `porcelain_deps` hack:
        #
        #   - For performance & capacity reasons, some of our CI target
        #     determinators will only look for tests separated by a limited
        #     number of dependency edges from the modified source (four, as
        #     of Jul 2019).
        #
        #     Using `porcelain_deps` puts the test code just one extra edge
        #     away from the test sources, which will normally guarantee that
        #     CI will run up the test when appropriate.
        #
        #   - In languages like C++, further indirection is required, which
        #     actually **breaks** the runtime dependency between the
        #     porcelain target and the wrapper implementation.  Buck
        #     provides no mechanism for expressing runtime dependencies in
        #     genrules, but `porcelain_deps` papers over this issue.  NB:
        #     Using `wrap_runtime_deps` is not appropriate to fix this
        #     issue, since that would produce uncacheable `cpp_unittest`
        #     builds, but Buck's underlying `cxx_test` lacks a `cacheable`
        #     property.
        porcelain_deps = [
            # Make the porcelain's dependency on the user-visible inputs as
            # direct as possible.
            inner_test_target,
            layer,

            # Tell CI determinators to trigger all container tests if the
            # underlying wrapper implementation changes.
            #
            # Not adding `test_layer`, or `wrapper_impl_library`, or
            # `test_spec_py`, since their internals would only change if
            # `:image_unittest_helpers` changes.
            caller_fake_library,  # Should depend on `:image_unittest_helpers`

            # Future: This currently lacks a direct dependency on
            # `nspawn_in_subvol/run_test.py` & friends, but adding that
            # dependency via `//fs_image/nspawn_in_subvol:run-test` would
            # force builds to wait for that unnecessary PAR to be built.
            # Leaving it out for now, we can change our mind if our risk vs
            # speed assessment changes.
        ],
    )

image_unittest_helpers = struct(
    hidden_test_name = _hidden_test_name,
    nspawn_wrapper_properties = _nspawn_wrapper_properties,
    tags_to_hide_test = _tags_to_hide_test,
)
