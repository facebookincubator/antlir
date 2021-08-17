# Copyright (c) Facebook, Inc. and its affiliates.
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

load("@bazel_skylib//lib:shell.bzl", "shell")
load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:image_unittest_helpers.bzl", helpers = "image_unittest_helpers")
load("//antlir/bzl:oss_shim.bzl", "buck_sh_test", "cpp_unittest", "python_unittest", "rust_unittest")
load("//antlir/bzl:shape.bzl", "shape")
load(":build_vm_run_target.bzl", "build_vm_run_target")
load(":types.bzl", "api")

_RULE_TO_TEST_TYPE = {
    cpp_unittest: "gtest",
    python_unittest: "pyunit",
    rust_unittest: "rust",
}
_RULE_TO_TEST_TAG = {
    cpp_unittest: "cpp",
    python_unittest: "python",
    rust_unittest: "rust",
}

def _build_test_tags(unittest_rule, tags):
    """
    Convert top-level 'tags' kwargs into separate tag sets for the outer and
    inner test rules.

    'tags' provided by a user are always applied to the outer test, so they
    control the behavior of TestPilot or to add information for 'buck query'.
    """
    wrapper_tags = tags + ["vmtest", "heavyweight"]

    # Make sure that the test runner ignores the underlying test, and only
    # looks at the version that runs in a VM.
    inner_tags = helpers.tags_to_hide_test()

    # Due to a complex internal migration, these tags are required to both
    # change the runtime behavior of the outer test, as well as build-time
    # behavior of the inner target.
    if unittest_rule == python_unittest:
        wrapper_tags.append("use-testpilot-adapter")

        # this tag gets added to the inner test automatically, but we must
        # inform tpx that the wrapper observes the same behavior
        wrapper_tags.append("tpx:list-format-migration:json")
        inner_tags.append("use-testpilot-adapter")

    return inner_tags, wrapper_tags

def _vm_unittest(
        name,
        unittest_rule,
        vm_opts = None,
        visibility = None,
        env = None,
        # vmtest target graphs tend to be very deep, since they invoke multiple
        # layers of images, kernel targets as well as the inner test target
        # user-specified deps end up attached only to the inner test target,
        # which frequently cause Sandcastle to skip running vmtest since the
        # resulting outer test target (that actually runs a VM and the tests
        # inside) is too far away from the user-given deps see D19499568 and
        # linked discussions for more details
        user_specified_deps = None,
        # Provide a mechanism for users to control running all the test cases
        # defined in a single unittest as a bundle.  Running as a bundle means
        # that only *one* VM instance will be spun up for the whole unittest
        # and all test cases will be executed inside that single VM instance.
        # This might have undesirable effects if the test case is intentionally
        # doing something that changes the state of the VM that cannot or
        # should not be undone by the test fixture (ie, rebooting or setting
        # a sysctl that cannot be undone for example).
        run_as_bundle = False,
        **kwargs):
    if kwargs.pop("layer", None):
        fail("Please provide the `layer` attribute as part of `vm_opts`.")

    if kwargs.pop("kernel", None):
        fail("Please provide the `kernel` attribute as part of `vm_opts`.")

    # Set some defaults
    env = env or {}
    vm_opts = vm_opts or api.opts.new()

    # Construct tags for controlling/influencing the unittest runner.
    # Future: These tags are heavily FB specific and really have no place
    # in the OSS side.  It would be nice if these weren't blindly applied.
    actual_test_tags, wrapper_tags = _build_test_tags(unittest_rule, kwargs.pop("tags", []))
    wrapper_tags.append("vmtest_" + _RULE_TO_TEST_TAG[unittest_rule])
    if run_as_bundle:
        wrapper_tags.append("run_as_bundle")

    # Build the actual unit test binary/target here
    actual_test_binary = helpers.hidden_test_name(name)
    if unittest_rule == rust_unittest:
        # otherwise the linter complains that the crate is not snake_case
        actual_test_binary = actual_test_binary.lower().replace("--", "-").replace("__", "_")
        kwargs["labels"] = actual_test_tags  # no tags in `rust_unittest`
    else:
        kwargs["tags"] = actual_test_tags
    unittest_rule(
        name = actual_test_binary,
        visibility = [],
        antlir_rule = "user-internal",
        **kwargs
    )

    # Build an image layer + package containing the actual test binary
    actual_test_layer = "{}__test-binary-layer".format(name)
    image.layer(
        name = actual_test_layer,
        features = [
            image.install_buck_runnable(":" + actual_test_binary, "/test"),
            image.install_buck_runnable("//antlir/vm:wrap-in-vm-test-exec", "/wrap"),
        ],
        flavor = REPO_CFG.antlir_linux_flavor,
    )

    actual_test_image = "{}__test-binary-image".format(name)
    image.package(
        name = actual_test_image,
        format = "btrfs",
        layer = ":" + actual_test_layer,
        loopback_opts = image.opts(
            # Do not try and optimize this when building in mode/opt
            minimize_size = False,
        ),
    )

    run_target = build_vm_run_target(
        name = "{}=vmtest".format(name),
        args = [
            "--test-binary $(location {})".format(shell.quote(":" + actual_test_binary)),
            "--test-binary-image $(location {})".format(shell.quote(":" + actual_test_image)),
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
        ] + ([
            # Future: This devel layer is just another mount to configure the VM with.
            # it's not special except that we don't hvae clean abstraction (yet) to
            # provide aribtrary mounts that should be setup by the VM.  For now we
            # provide this flag so the vmtest binary can setup the devel mount.
            "--devel-layer",
        ] if vm_opts.devel else []),
        exe_target = "//antlir/vm:vmtest",
        vm_opts = vm_opts,
    )

    # Building buck_sh_test with a specific type to trick TestPilot into
    # thinking that it is running a unit test of the specific type directly.
    # In reality {}--vmtest-binary will transparently run the binary in a VM
    # and buck will write the given type into the external_runner_spec.json that
    # is given to TestPilot
    buck_sh_test(
        name = name,
        labels = wrapper_tags,
        test = ":" + run_target,
        type = _RULE_TO_TEST_TYPE[unittest_rule],
        visibility = visibility,
        # Although the wrapper test doesn't actually need these dependencies,
        # we add this to reduce the dependency distance from outer test target
        # to the libraries that the inner test target depends on. Reducing the
        # dependency distance maximizes the chances that CI will kick off the
        # outer test target when deps change.
        deps = user_specified_deps,
        # TPX is unaware of the inner test binary, so it must be informed of
        # its location for things that need to inspect the actual inner test
        # binary, like llvm-cov
        env = {"BUCK_BASE_BINARY": "$(location :{})".format(actual_test_binary)},
        antlir_rule = "user-facing",
    )

def _vm_cpp_unittest(
        name,
        vm_opts = None,
        deps = (),
        **kwargs):
    _vm_unittest(
        name,
        cpp_unittest,
        vm_opts = vm_opts,
        deps = deps,
        user_specified_deps = deps,
        **kwargs
    )

def _vm_python_unittest(
        name,
        kernel = None,
        vm_opts = None,
        **kwargs):
    # Short circuit the target graph by attaching user_specified_deps to the outer
    # test layer.
    user_specified_deps = []
    user_specified_deps += kwargs.get("deps", [])
    user_specified_deps += kwargs.get("runtime_deps", [])
    resources = kwargs.get("resources", [])
    if types.is_dict(resources):
        resources = list(resources.keys())
    user_specified_deps += resources

    _vm_unittest(
        name,
        python_unittest,
        vm_opts = vm_opts,
        user_specified_deps = user_specified_deps,
        # unittest pars must be xars so that native libs work inside the vm without
        # linking / mounting trickery
        par_style = "xar",
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
        kernels,
        vm_opts = None,
        **kwargs):
    for suffix, kernel in kernels.items():
        if vm_opts:
            merged_vm_opts = shape.as_dict_shallow(vm_opts)
            merged_vm_opts["kernel"] = kernel

            # Don't provide the initrd originally constructed since
            # the kernel version likely changed
            merged_vm_opts.pop("initrd")
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
        **kwargs):
    _vm_multi_kernel_unittest(
        name,
        _vm_cpp_unittest,
        **kwargs
    )

def _vm_multi_kernel_python_unittest(
        name,
        **kwargs):
    _vm_multi_kernel_unittest(
        name,
        _vm_python_unittest,
        **kwargs
    )

def _rootfs_disk_rc():
    return api.disk.new(
        package = REPO_CFG.artifact["vm.rootfs.btrfs.rc"],
    )

def rootfs_disk_stable():
    return api.disk.new(
        package = REPO_CFG.artifact["vm.rootfs.btrfs.stable"],
    )

vm = struct(
    cpp_unittest = _vm_cpp_unittest,
    # This nested structure is for looking up the default set of artifacts
    # used for this subsystem.
    artifacts = struct(
        rootfs = struct(
            layer = struct(
                rc = REPO_CFG.artifact["vm.rootfs.layer.rc"],
                stable = REPO_CFG.artifact["vm.rootfs.layer.stable"],
            ),
            disk = struct(
                rc = _rootfs_disk_rc,
                stable = rootfs_disk_stable,
            ),
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
