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

- A user can provide a `vm_opts` struct which controls how the VM is
  configured at runtime.  The `vm_opts` struct is created with the `vm.opts`
  function and has the following form:

  ```
  vm_opts = vm.opts(
      # This boolean option will control the install of both the kernel headers
      # and sources for the kernel version the unittest is configured to use.
      devel = False,

      # The number of Virtual CPUs to provide the VM.
      ncpus = 1,
  )
  ```

- Currently, `run_as_user` and `hostname` are not supported by VM tests.
  There are plans to add support for these options (see T62319183).

- The `boot` option does not affect `vm.{cpp,python}_unittest` as there is
  no way to run VM tests in non-booted mode.
"""

load("@bazel_skylib//lib:shell.bzl", "shell")
load("@bazel_skylib//lib:types.bzl", "types")
load(":image.bzl", "image")
load(":image_unittest_helpers.bzl", helpers = "image_unittest_helpers")
load(":oss_shim.bzl", "buck_genrule", "buck_sh_test", "cpp_unittest", "default_vm_image", "get_visibility", "kernel_get", "python_binary", "python_unittest")

_RULE_TO_TEST_TYPE = {
    cpp_unittest: "gtest",
    python_unittest: "pyunit",
}

def _create_vm_opts(
        devel = False,
        ncpus = 1):
    if ncpus == 2:
        fail("ncpus=2 will cause kernel panic: https://fburl.com/md27i5k8")

    return struct(
        devel = devel,
        ncpus = ncpus,
    )

def _tags(unittest_rule, kwargs):
    """
    Convert top-level 'tags' kwargs into separate tag sets for the outer and
    inner test rules.

    'tags' provided by a user are always applied to the outer test, so they
    control the behavior of TestPilot or to add information for 'buck query'.
    """
    outer_tags = kwargs.get("tags", []) + ["vmtest"]

    # Make sure that the test runner ignores the underlying test, and only
    # looks at the version that runs in a VM.
    inner_tags = helpers.tags_to_hide_test()

    # Due to a complex internal migration, these tags are required to both
    # change the runtime behavior of the outer test, as well as build-time
    # behavior of the inner target.
    if unittest_rule == python_unittest:
        outer_tags.append("use-testpilot-adapter")
        inner_tags.append("use-testpilot-adapter")
    if unittest_rule == cpp_unittest:
        outer_tags.append("tpx-test-type:vmtest_gtest")

    return inner_tags, outer_tags

def _inner_test(
        name,
        unittest_rule,
        inner_tags,
        **kwargs):
    inner_test_name = helpers.hidden_test_name(name)
    unittest_rule(
        name = inner_test_name,
        tags = inner_tags,
        visibility = [],
        **kwargs
    )
    return inner_test_name

def _outer_test(
        name,
        unittest_rule,
        inner_test,
        inner_test_package,
        seed_device,
        kernel,
        visibility,
        tags,
        user_specified_deps,
        env,
        vm_opts):
    visibility = get_visibility(visibility, name)

    if not env:
        env = {}

    python_binary(
        name = "{}=runvm".format(name),
        base_module = "antlir.vm",
        antlir_rule = "user-internal",
        main_module = "antlir.vm.vmtest",
        par_style = "xar",
        resources = {
            seed_device: "image",
            inner_test_package: "test.btrfs",
            # the inner_test here is used for discovery only, the actual test
            # binary is installed into the vm with
            # `image.install_buck_runnable`
            inner_test: "test_discovery_binary",
        },
        visibility = [],
        deps = [
            "//antlir/vm:vmtest",
            "//antlir/vm/kernel:{}-vm".format(kernel.uname),
        ],
    )

    # Build an executable script that collects all the options passed to the
    # vmtest binary.  This provides a way to manually execute the vmtest script
    # that is invoked by the test runner directly.
    # Future: Expand this to provide support for other executable entry points
    #         for running vms.  ie: as something like a -run-vm target for a
    #         kernel or an image.
    buck_genrule(
        name = "{}=vmtest".format(name),
        out = "run",
        bash = """
cat > "$TMP/out" << 'EOF'
#!/bin/sh
set -ue -o pipefail -o noclobber
exec $(exe {vm_binary_target}) \
  {setenv_quoted} \
  {ncpus} \
  "$@"
EOF
chmod +x "$TMP/out"
mv "$TMP/out" "$OUT"
        """.format(
            # Manually extract any environment variables set and format
            # them into `--setenv NAME=VALUE`. THese are passed during the call to
            # vmtest which will forward them inside the vm for the inner test.
            setenv_quoted = " ".join([
                "--setenv={}".format(
                    shell.quote(
                        "{}={}".format(
                            var_name,
                            var_value,
                        ),
                    ),
                )
                for var_name, var_value in env.items()
            ]),
            ncpus = "--ncpus={}".format(vm_opts.ncpus),
            vm_binary_target = ":{}=runvm".format(name),
        ),
        cacheable = False,
        executable = True,
        visibility = [],
        antlir_rule = "user-internal",
    )

    # building a buck_sh_test with a specific type lets us trick TestPilot into
    # thinking that it is running a unit test of the specific type directly,
    # when in reality vmtest.par will transparently run the binary in a VM
    # buck will write the given type into the external_runner_spec.json that is
    # given to TestPilot
    buck_sh_test(
        name = name,
        labels = tags,
        test = ":{}=vmtest".format(name),
        type = _RULE_TO_TEST_TYPE[unittest_rule],
        visibility = visibility,
        # Although the outer test doesn't actually need these dependencies,
        # we add this to reduce the dependency distance from outer test target
        # to the libraries that the inner test target depends on. Reducing the
        # dependency distance maximizes the chances that CI will kick off the
        # outer test target when deps change. See D19499568 and linked
        # discussions for more details.
        deps = user_specified_deps,
        # TPX is unaware of the inner test binary, so it must be informed of
        # its location for things that need to inspect the actual inner test
        # binary, like llvm-cov
        env = {"BUCK_BASE_BINARY": "$(location {})".format(inner_test)},
        antlir_rule = "user-facing",
    )

def _vm_unittest(
        name,
        unittest_rule,
        kernel = None,
        layer = None,
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
    inner_tags, outer_tags = _tags(unittest_rule, kwargs)
    kwargs.pop("tags", None)

    inner_test = _inner_test(
        name,
        unittest_rule,
        inner_tags,
        antlir_rule = "user-internal",
        **kwargs
    )

    kernel = kernel or kernel_get.default
    layer = layer or default_vm_image.layer
    vm_opts = vm_opts or _create_vm_opts()

    # Create an image layer and package that contains the test binary. This package
    # will be used as a device for the VM and mounted at `/vmtest`
    image.layer(
        name = "{}_test_layer".format(name),
        features = [
            image.install_buck_runnable(":" + inner_test, "/test"),
        ],
    )
    image.package(
        name = "{}=test.btrfs".format(name),
        layer = ":{}_test_layer".format(name),
    )

    features = []

    # some tests need kernel headers and kernel sources (bcc on kernels <5.2), so provide
    # an opt-in way to install them in the test image, instead of forcing it
    # for every test.
    if vm_opts.devel:
        features.append(
            image.rpms_install([kernel.headers, kernel.devel]),
        )

    # if there are no custom image features and the test is using the default
    # rootfs layer, we can use the pre-packaged seed device and save lots of
    # build time
    seed_device = default_vm_image.package
    if features or layer != default_vm_image.layer:
        image.layer(
            name = "{}-image".format(name),
            parent_layer = layer,
            features = features,
        )
        layer = ":{}-image".format(name)

        seed_device = "{}=seed.btrfs".format(name)
        image.package(
            name = seed_device,
            layer = ":{}-image".format(name),
            seed_device = True,
            writable_subvolume = True,
            visibility = [],
            antlir_rule = "user-internal",
        )
        seed_device = ":" + seed_device

    _outer_test(
        name,
        env = env or {},
        inner_test = ":" + inner_test,
        inner_test_package = ":{}=test.btrfs".format(name),
        kernel = kernel,
        seed_device = seed_device,
        tags = outer_tags,
        unittest_rule = unittest_rule,
        user_specified_deps = user_specified_deps,
        visibility = visibility,
        vm_opts = vm_opts,
    )

def _vm_cpp_unittest(
        name,
        kernel = None,
        layer = None,
        vm_opts = None,
        deps = (),
        **kwargs):
    _vm_unittest(
        name,
        cpp_unittest,
        kernel = kernel,
        layer = layer,
        vm_opts = vm_opts,
        deps = deps,
        user_specified_deps = deps,
        **kwargs
    )

def _vm_python_unittest(
        name,
        kernel = None,
        layer = None,
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
        kernel = kernel,
        layer = layer,
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
        vm_unittest(
            name = "-".join([name, suffix]),
            kernel = kernel,
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
    # The set of reasonable defaults for running vms
    default = struct(
        kernel = kernel_get.default,
        layer = default_vm_image.layer,
    ),
    cpp_unittest = _vm_cpp_unittest,
    # An API for constructing a set of tests that are all the
    # same except for the kernel version.
    multi_kernel = struct(
        cpp_unittest = _vm_multi_kernel_cpp_unittest,
        python_unittest = _vm_multi_kernel_python_unittest,
    ),
    python_unittest = _vm_python_unittest,
    opts = _create_vm_opts,
)
