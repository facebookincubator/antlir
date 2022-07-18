# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:oss_shim.bzl", "buck_command_alias")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load(":container_opts.bzl", "normalize_container_opts")
load(":image_layer.bzl", "image_layer")
load(":image_layer_runtime.bzl", "container_target_name", "systemd_target_name")
load(":oss_shim.bzl", "buck_genrule", "python_library")
load(":query.bzl", "layer_deps_query")
load(":snapshot_install_dir.bzl", "snapshot_install_dir")
load(":structs.bzl", "structs")
load(":target_helpers.bzl", "normalize_target", "targets_and_outputs_arg_list")

def _hidden_test_name(name, test_type = None):
    # This is the test binary that is supposed to run inside the image.
    if test_type == "rust":
        # without special casing rust names, the rust compiler complains that
        # the crate name has two adjacent underscores and is not proper
        # snake_case, so give in to its demands
        return name + "_test_binary"
    return name + "__test_binary"

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
        visibility,
        hostname,
        # An `image.opts` containing keys from `container_opts_t`.
        # If you want to install packages, you will usually want to
        # set `shadow_proxied_binaries`.
        container_opts):
    container_opts = normalize_container_opts(container_opts)

    # Fail early, so the user doesn't have to wait for the test to build.
    # Future: we could potentially relax this since some odd applications
    # might want to just talk to the repo-server, and not install RPMs.
    if run_as_user != "root":
        if container_opts.serve_rpm_snapshots:
            fail(
                'Needs `run_as_user = "root"` to install RPMs',
                "container_opts.serve_rpm_snapshots",
            )
        if container_opts.shadow_proxied_binaries:
            fail(
                "All binaries now shadowed by `shadow_proxied_binaries` " +
                'require `run_as_user = "root"`',
                "container_opts.shadow_proxied_binaries",
            )

    # Since we do not pass the `shape` directly into `NspawnPluginArgs`,
    # plumbing has to be added for each new option.  On the bright side,
    # this lets us decide whether an option even makes sense in unit tests.
    unsupported_opts = [
        k
        for k in structs.to_dict(container_opts).keys()
        if not k.startswith("_") and not k in [
            "attach_antlir_dir",
            "boot_await_dbus",
            "internal_only_logs_tmpfs",
            "serve_rpm_snapshots",
            "shadow_paths",
            "shadow_proxied_binaries",
            "proxy_server_config",
            "internal_only_allow_mknod",
            "internal_only_unprotect_antlir_dir",  # Unavailable in tests
            "internal_only_bind_repo_ro",
            "internal_only_bind_artifacts_dir_rw",
        ]
    ]
    if unsupported_opts:
        fail(
            "Not yet implemented: a small amount of plumbing is needed to " +
            "enable these with `image.*_unittest`: {}".format(unsupported_opts),
            "container_opts",
        )
    if container_opts.internal_only_unprotect_antlir_dir:
        fail("`internal_only_unprotect_antlir_dir` is not allowed in tests")
    if container_opts.attach_antlir_dir:
        fail("`attach_antlir_dir` is not yet implemented in tests")

    # These args must be on the outer wrapper test, regardless of language.
    outer_kwarg_names = ["tags", "labels", "env"]
    outer_kwarg_names.extend(extra_outer_kwarg_names)

    outer_test_kwargs = {k: v for k, v in inner_test_kwargs.items() if k in outer_kwarg_names}
    inner_test_kwargs = {k: v for k, v in inner_test_kwargs.items() if k not in outer_test_kwargs}

    # This target name gets a suffix to keep it discoverable via tab-completion
    test_layer = name + "__test_layer"

    # Make a test-specific image containing the test binary.
    binary_path = "/layer-test-binary.par"
    inner_test_target = ":" + _hidden_test_name(name, test_type)
    if test_type == "rust":
        # for the rust linter warning that the crate is not snake_case
        inner_test_target = inner_test_target.lower().replace("--", "-")
    image_layer(
        name = test_layer,
        parent_layer = layer,
        features = [feature.install_buck_runnable(inner_test_target, binary_path)],
        visibility = visibility,
        runtime = ["systemd"] if boot else [],
    )

    # For ergonomics, export the debug targets from the test's layer on the test
    buck_command_alias(
        name = container_target_name(name),
        antlir_rule = "user-internal",
        exe = ":" + container_target_name(test_layer),
    )
    if boot:
        buck_command_alias(
            name = systemd_target_name(name),
            antlir_rule = "user-internal",
            exe = ":" + systemd_target_name(test_layer),
        )

    # Generate a `.py` file that sets some of the key container options.
    #
    # NB: It would have been possible to use `env` to pass the arguments and
    # the location of the test layer to the driver binary.  However, this
    # would prevent one from running the test binary directly, bypassing
    # Buck.  Since Buck CLI is slow, this would be a significant drop in
    # usability, so we use this library trick.
    test_spec_py = name + "__layer-test-spec-py"
    buck_genrule(
        name = test_spec_py,
        bash = """
cat > "$TMP/out" << EOF
import os
TEST_TYPE={test_type_repr}
def nspawn_in_subvol_args():
    return [
        *(['--debug'] if os.environ.get('ANTLIR_DEBUG') else []),
        *[{maybe_user}],
        *[
            '--setenv={{}}={{}}'.format(k, os.environ.get(k, ''))
                for k in {pass_through_env_repr}
        ],
        *[{maybe_allow_mknod}],
        *[{maybe_boot}],
        *[{maybe_boot_no_await_dbus}],
        *[{maybe_hostname}],
        {maybe_logs_tmpfs}
        {maybe_no_shadow_proxied_binaries}
        *[
            '--serve-rpm-snapshot={{}}'.format(s)
                for s in {serve_rpm_snapshots_repr}
        ],
        *[{shadow_paths_repr}],
        *{targets_and_outputs},
        '--append-console',
        '--setenv=ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP=1',
        '--attach-antlir-dir-mode=off',
        {maybe_bind_repo_ro}
        {maybe_bind_artifacts_dir_rw}
        '--', {binary_path_repr},
    ]
EOF
mv $TMP/out "$OUT"
        """.format(
            binary_path_repr = repr(binary_path),
            maybe_allow_mknod = "'--allow-mknod'" if container_opts.internal_only_allow_mknod else "",
            maybe_boot = "'--boot'" if boot else "",
            maybe_boot_no_await_dbus = "'--boot-no-await-dbus'" if not container_opts.boot_await_dbus else "",
            maybe_hostname = "'--hostname={hostname}'".format(hostname = hostname) if hostname else "",
            # The next 3 would be nice to pass as `container_opts_t`, but
            # this entire interface is pinned to the `nspawn_in_subvol` CLI,
            # so this manual marshaling is the path of least resistance.  In
            # the future, one could consider `shape`ifying this interface.
            maybe_logs_tmpfs = (
                "'--logs-tmpfs'," if container_opts.internal_only_logs_tmpfs else ""
            ),
            maybe_no_shadow_proxied_binaries = (
                "" if container_opts.shadow_proxied_binaries else "'--no-shadow-proxied-binaries',"
            ),
            maybe_bind_repo_ro = (
                "'--bind-repo-ro'," if container_opts.internal_only_bind_repo_ro else ""
            ),
            maybe_bind_artifacts_dir_rw = (
                "'--bind-artifacts-dir-rw'," if container_opts.internal_only_bind_artifacts_dir_rw else ""
            ),
            pass_through_env_repr = repr(outer_test_kwargs.get("env", [])),
            serve_rpm_snapshots_repr = repr([
                snapshot_install_dir(s)
                for s in container_opts.serve_rpm_snapshots
            ]),
            shadow_paths_repr = ", ".join([
                "'--shadow-paths', {}, {}".format(repr(sp.dst), repr(sp.src))
                for sp in container_opts.shadow_paths
            ]),
            targets_and_outputs = targets_and_outputs_arg_list(
                name = name,
                query = layer_deps_query(
                    layer = normalize_target(":" + test_layer),
                ),
            ),
            test_type_repr = repr(test_type),
            maybe_user = (
                "'--user={user}'".format(user = run_as_user) if run_as_user else ""
            ),
        ),
        visibility = visibility,
        antlir_rule = "user-internal",
        cacheable = False,
    )

    # To execute the wrapped test, the caller must make this library's
    # `antlir.nspawn_in_subvol.run_test` the `main_module` of a
    # `python_binary`, and arrange for the binary to be run as if it were of
    # the inner test type.
    #
    # IMPORTANT: If you add more dependencies to THIS LIBRARY, or to any of
    # the AUXILIARY TARGETS above, you must add their externally observable
    # parts to `porcelain_deps` below, or CI test triggering will break.
    wrapper_impl_library = "{}__layer-test-wrapper-library".format(name)
    python_library(
        name = wrapper_impl_library,
        visibility = visibility,

        # This library puts the following files under
        # `antlir/nspawn_in_subvol` in the source archive:
        #  - `run_test.py` with the business logic
        #  - `test_layer` from above
        #  - `test_spec_py` from above
        # This makes it easy for `nspawn_test_in_subvol()` to find its data.
        base_module = "antlir.nspawn_in_subvol",
        deps = ["//antlir/nspawn_in_subvol:run-test-library"],
        resources = {":" + test_layer: "nspawn-in-test-subvol-layer"},
        srcs = {":" + test_spec_py: "__image_python_unittest_spec__.py"},
        antlir_rule = "user-internal",
        tags = ["no_pyre"],
    )

    return struct(
        inner_test_kwargs = inner_test_kwargs,
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

            # Future: This currently lacks a direct dependency on
            # `nspawn_in_subvol/run_test.py` & friends, but adding that
            # dependency via `//antlir/nspawn_in_subvol:run-test` would
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
