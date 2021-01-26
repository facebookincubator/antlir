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
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:image_unittest_helpers.bzl", helpers = "image_unittest_helpers")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "buck_sh_test", "cpp_unittest", "default_vm_image", "python_unittest")
load("//antlir/bzl:shape.bzl", "shape")
load(":types.bzl", "api")

_RULE_TO_TEST_TYPE = {
    cpp_unittest: "gtest",
    python_unittest: "pyunit",
}

def _build_run_target(
        # The name of the runnable target
        name,
        # An instance of a vm_opts_t shape.
        vm_opts,
        # A list of additional cli args to pass to the provided exe_target.
        # This is passed directly to the `exe_target` so they should already be
        # properly formatted.
        args = None,
        # The exe target to execute.
        exe_target = "//antlir/vm:run"):
    buck_genrule(
        name = name,
        out = "run",
        bash = """
cat > "$TMP/out" << 'EOF'
#!/bin/sh
set -ue -o pipefail -o noclobber
exec $(exe {exe_target}) \
--opts {opts_quoted} \
{extra_args} \
"$@"
EOF
chmod +x "$TMP/out"
mv "$TMP/out" "$OUT"
        """.format(
            exe_target = exe_target,
            extra_args = " ".join(args) if args else "",
            opts_quoted = shell.quote(shape.do_not_cache_me_json(
                instance = vm_opts,
                shape = api.opts.t,
            )),
        ),
        cacheable = False,
        executable = True,
        visibility = [],
        antlir_rule = "user-internal",
    )

    return name

def _build_test_tags(unittest_rule, tags):
    """
    Convert top-level 'tags' kwargs into separate tag sets for the outer and
    inner test rules.

    'tags' provided by a user are always applied to the outer test, so they
    control the behavior of TestPilot or to add information for 'buck query'.
    """
    wrapper_tags = tags + ["vmtest"]

    # Make sure that the test runner ignores the underlying test, and only
    # looks at the version that runs in a VM.
    inner_tags = helpers.tags_to_hide_test()

    # Due to a complex internal migration, these tags are required to both
    # change the runtime behavior of the outer test, as well as build-time
    # behavior of the inner target.
    if unittest_rule == python_unittest:
        wrapper_tags.append("use-testpilot-adapter")
        inner_tags.append("use-testpilot-adapter")
    if unittest_rule == cpp_unittest:
        wrapper_tags.append("tpx-test-type:vmtest_gtest")

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

    # Build the actual unit test binary/target here
    actual_test_binary = helpers.hidden_test_name(name)
    unittest_rule(
        name = actual_test_binary,
        tags = actual_test_tags,
        visibility = [],
        antlir_rule = "user-internal",
        **kwargs
    )

    # Build an image layer + package containing the actual test binary
    actual_test_layer = "{}--test-binary-layer".format(name)
    actual_test_image = "{}=image.btrfs".format(actual_test_layer)
    image.layer(
        name = actual_test_layer,
        features = [
            image.install_buck_runnable(":" + actual_test_binary, "/test"),
        ],
    )
    image.package(
        name = actual_test_image,
        layer = ":" + actual_test_layer,
    )

    run_target = _build_run_target(
        name = "{}=vmtest".format(name),
        args = [
            "--test-binary $(location {})".format(shell.quote(":" + actual_test_binary)),
            "--test-binary-image $(location {})".format(shell.quote(":" + actual_test_image)),
            # Always enable debug + console logging for better debugging
            "--debug",
            "--console",
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

def _vm_multi_kernel_unittest(
        name,
        vm_unittest,
        kernels,
        vm_opts = None,
        **kwargs):
    for suffix, kernel in kernels.items():
        if vm_opts:
            merged_vm_opts = shape.as_dict(vm_opts)
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

vm = struct(
    cpp_unittest = _vm_cpp_unittest,
    # The set of reasonable defaults for running vms
    default = struct(
        layer = default_vm_image.layer,
    ),
    # An API for constructing a set of tests that are all the
    # same except for the kernel version.
    multi_kernel = struct(
        cpp_unittest = _vm_multi_kernel_cpp_unittest,
        python_unittest = _vm_multi_kernel_python_unittest,
    ),
    python_unittest = _vm_python_unittest,
    # API export for building vm_opt_t and related types
    types = api,
    run = _build_run_target,
)
