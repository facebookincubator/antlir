# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Similar to the image.{cpp,python}_unittest macros, the intent of
vm.{cpp,python}_unittest is to be able to run unittests inside
a specific antlir layer. The main difference is that the macros in this
file will run the tests inside a fully booted VM instead of inside a
systemd-nspawn container.

The interface of vm.{cpp,python}_unittest has been designed to be
similar to that of image.{cpp,python}_unittest; therefore, one should
first look over the interface specified by those macros to become familiar
with the options allowed there.  The key differences with
`vm.{cpp,python}_unittest` are:

- A `kernel` attribute can optionally be provided to explicitly choose a
  non-default kernel version to run the VM.  The `kernel` attribute is
  a struct containing various attributes and target locations for artifacts.

- A `layer` attribute can optionally be provided to explicitly choose a
  non-default `image.layer` to boot the VM with and run the test.  Providing
  a non-default `image.layer` will incur additional build cost due to the need
  to consturct a btrfs seed device.  As such, if you can avoid a custom
  `image.layer`, it would be ideal.
  Note: this `image.layer` *must* be capable of successfully booting the
  VM for the tests to run.

- A user can provide a `vm_opts` shape which controls how the VM is
  configured at runtime.  The `vm_opts` shape is created with the `vm.opts`
  function and has the following form:

  ```
  vm_opts = vm.opts(
      # This boolean option will control the install of both the kernel headers
      # and sources for the kernel version the unittest is configured to use.
      devel = False,

      # The number of Virtual CPUs to provide the VM.
      cpus = 1,

      # The amount of memory, in mb, to provide to the VM.
      mem_mb = 4096,

      # The rootfs image to use for the vm, specified as a buck target
      rootfs_image = "//buck/target/path:image",
  )
  ```

- Currently, `run_as_user` and `hostname` are not supported by VM tests.
  There are plans to add support for these options (see T62319183).

- The `boot` option does not affect `vm.{cpp,python}_unittest` as there is
  no way to run VM tests in non-booted mode.
"""

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:build_defs.bzl", "add_test_framework_label", "buck_sh_test", "cpp_unittest", "python_unittest", "rust_unittest")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image_unittest_helpers.bzl", helpers = "image_unittest_helpers")
load("//antlir/bzl:kernel_shim.bzl", "kernels")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load(":build_vm_run_target.bzl", "build_vm_run_target")
load(":types.bzl", "api")

_RULE_TO_TEST_TYPE = {
    cpp_unittest: "gtest",
    python_unittest: "pyunit",
    rust_unittest: "rust",
}
_RULE_TO_TEST_LABEL = {
    cpp_unittest: "cpp",
    python_unittest: "python",
    rust_unittest: "rust",
}

def _build_test_labels(unittest_rule, labels):
    """
    Convert top-level 'labels' into separate label sets for the outer and
    inner test rules.

    'labels' provided by a user are always applied to the outer test, so they
    control the behavior of TestPilot or to add information for 'buck query'.
    """
    wrapper_labels = labels + ["vmtest", "heavyweight"]

    # Make sure that the test runner ignores the underlying test, and only
    # looks at the version that runs in a VM.
    inner_labels = helpers.tags_to_hide_test()

    # Due to a complex internal migration, these labels are required to both
    # change the runtime behavior of the outer test, as well as build-time
    # behavior of the inner target.
    if unittest_rule == python_unittest:
        wrapper_labels.append("use-testpilot-adapter")

        # this tag gets added to the inner test automatically, but we must
        # inform tpx that the wrapper observes the same behavior
        wrapper_labels.append("tpx:list-format-migration:json")
        inner_labels.append("use-testpilot-adapter")

        # annotate both inner and wrapper target with a framework
        wrapper_labels = add_test_framework_label(wrapper_labels, "test-framework=8:vmtest")
        inner_labels = add_test_framework_label(inner_labels, "test-framework=8:vmtest")

    return inner_labels, wrapper_labels

def _vm_unittest(
        name,
        unittest_rule,
        vm_opts = None,
        visibility = None,
        env = None,
        # Provide a mechanism for users to control running all the test cases
        # defined in a single unittest as a bundle.  Running as a bundle means
        # that only *one* VM instance will be spun up for the whole unittest
        # and all test cases will be executed inside that single VM instance.
        # This might have undesirable effects if the test case is intentionally
        # doing something that changes the state of the VM that cannot or
        # should not be undone by the test fixture (ie, rebooting or setting
        # a sysctl that cannot be undone for example).
        run_as_bundle = False,
        timeout_secs = None,
        **kwargs):
    # Set some defaults
    env = env or {}
    vm_opts = vm_opts or api.opts.new()

    # Construct labels for controlling/influencing the unittest runner.
    # Future: These labels are heavily FB specific and really have no place
    # in the OSS side.  It would be nice if these weren't blindly applied.
    actual_test_labels, wrapper_labels = _build_test_labels(
        unittest_rule,
        kwargs.pop("labels", []),
    )
    wrapper_labels.append("vmtest_" + _RULE_TO_TEST_LABEL[unittest_rule])
    if run_as_bundle:
        wrapper_labels.append("run_as_bundle")

    # Build the actual unit test binary/target here
    actual_test_binary = helpers.hidden_test_name(name)
    if unittest_rule == rust_unittest:
        # otherwise the linter complains that the crate is not snake_case
        actual_test_binary = actual_test_binary.lower().replace("--", "-").replace("__", "_")

    unittest_rule(
        name = actual_test_binary,
        visibility = [],
        antlir_rule = "user-internal",
        labels = actual_test_labels,
        **kwargs
    )

    run_target = build_vm_run_target(
        name = "{}=vmtest".format(name),
        args = [
            "--test-binary $(location {})".format(shell.quote(":" + actual_test_binary)),
            "--test-binary-wrapper $(location {})".format(antlir_dep("vm:wrap-in-vm-test-exec")),
            "--test-type {}".format(shell.quote(_RULE_TO_TEST_TYPE[unittest_rule])),
            # Always enable debug + console logging for better debugging
            # --append-console is a tricky one: it has to be before --debug so that any
            # test case name is not interpreted as a file to path for the console
            "--append-console",
            "--debug",
        ] + [
            # Manually extract any environment variables set and format
            # them into `--setenv NAME=VALUE`. THese are passed during the call to
            # vmtest which will forward them inside the vm for the inner test.
            "--setenv={}".format(
                shell.quote(
                    "{}={}".format(
                        var_name,
                        var_value,
                    ),
                ),
            )
            for var_name, var_value in env.items()
        ] + (
            [
                # Future: This devel layer is just another mount to configure the VM with.
                # it's not special except that we don't hvae clean abstraction (yet) to
                # provide aribtrary mounts that should be setup by the VM.  For now we
                # provide this flag so the vmtest binary can setup the devel mount.
                "--devel-layer",
            ] if vm_opts.devel else []
        ) + ([
            "--timeout={}".format(timeout_secs),
        ] if timeout_secs else []),
        exe_target = antlir_dep("vm:vmtest"),
        vm_opts = vm_opts,
    )

    # Building buck_sh_test with a specific type to trick TestPilot into
    # thinking that it is running a unit test of the specific type directly.
    # In reality {}--vmtest-binary will transparently run the binary in a VM
    # and buck will write the given type into the external_runner_spec.json that
    # is given to TestPilot
    buck_sh_test(
        name = name,
        labels = wrapper_labels,
        test = ":" + run_target,
        type = _RULE_TO_TEST_TYPE[unittest_rule],
        visibility = visibility,
        # TPX is unaware of the inner test binary, so it must be informed of
        # its location for things that need to inspect the actual inner test
        # binary, like llvm-cov
        env = {"BUCK_BASE_BINARY": "$(location :{})".format(actual_test_binary)},
        antlir_rule = "user-facing",
    )

def _vm_cpp_unittest(
        name,
        vm_opts = None,
        **kwargs):
    _vm_unittest(
        name,
        cpp_unittest,
        vm_opts = vm_opts,
        **kwargs
    )

def _vm_python_unittest(
        name,
        vm_opts = None,
        **kwargs):
    _vm_unittest(
        name,
        python_unittest,
        vm_opts = vm_opts,
        **kwargs
    )

def _vm_rust_unittest(
        name,
        vm_opts = None,
        **kwargs):
    _vm_unittest(name, rust_unittest, vm_opts = vm_opts, **kwargs)

def _vm_multi_kernel_unittest(
        name,
        vm_unittest,
        kernel_list,
        vm_opts = None,
        disks = None,
        disk = None,
        custom_rootfs_layer = None,
        **kwargs):
    if len(kernel_list) == 0:
        fail("{}: will not run on any kernels, check the selection query!".format(name))
    if disks or disk:
        fail("disk(s) are not allowed with multi_kernel tests")
    kernel_list = sets.to_list(sets.make(kernel_list))
    for uname in kernel_list:
        kernel = kernels.get(uname)
        suffix = uname
        if vm_opts:
            merged_vm_opts = shape.as_dict_shallow(vm_opts)
            merged_vm_opts["kernel"] = kernel

            # Don't provide the initrd originally constructed since the
            # kernel version has changed and it's now invalid
            merged_vm_opts.pop("initrd")

            # Same with disks
            merged_vm_opts.pop("disks")

            if custom_rootfs_layer:
                merged_vm_opts["disks"] = [api.disk.root(layer = custom_rootfs_layer, kernel = kernel)]

            vm_opts = api.opts.new(**merged_vm_opts)
        else:
            vm_opts = api.opts.new(kernel = kernel)

        vm_unittest(
            name = "-".join([name, suffix]),
            vm_opts = vm_opts,
            **kwargs
        )

def _vm_multi_kernel_cpp_unittest(
        name,
        kernels,
        **kwargs):
    _vm_multi_kernel_unittest(
        name,
        _vm_cpp_unittest,
        kernel_list = kernels,
        **kwargs
    )

def _vm_multi_kernel_python_unittest(
        name,
        kernels,
        **kwargs):
    _vm_multi_kernel_unittest(
        name,
        _vm_python_unittest,
        kernel_list = kernels,
        **kwargs
    )

vm = struct(
    cpp_unittest = _vm_cpp_unittest,
    # This nested structure is for looking up the default set of artifacts
    # used for this subsystem.
    artifacts = struct(
        rootfs = struct(
            layer = REPO_CFG.artifact["vm.rootfs.layer"],
        ),
    ),
    # An API for constructing a set of tests that are all the
    # same except for the kernel version.
    multi_kernel = struct(
        cpp_unittest = _vm_multi_kernel_cpp_unittest,
        python_unittest = _vm_multi_kernel_python_unittest,
    ),
    python_unittest = _vm_python_unittest,
    rust_unittest = _vm_rust_unittest,
    # API export for building vm_opt_t and related types
    types = api,
    run = build_vm_run_target,
)
