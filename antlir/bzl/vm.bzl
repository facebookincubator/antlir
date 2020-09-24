"""
Similar to the image.{cpp,python}_unittest macros, the intent of
vm.{cpp,python}_unittest is to be able to run unittests inside
a specific antlir layer. The main difference is that the macros in this
file will run the tests inside a VM, while image.{cpp,python}_unittest
will run the tests inside a systemd-nspawn container.

The interface of vm.{cpp,python}_unittest has been designed to be
similar to that of image.{cpp,python}_unittest; therefore, one should
first look over the interface specified by those macros. In fact, the
only key differences are:
- With vm.{cpp,python}_unittest, the user has the option to provide
  a `kernel_opts` value, which is a struct of the following format:
  ```
  kernel_opts = vm.opts(
      kernel = "//target/of/specific/kernel/version",
      install_headers = Bool,
      install_devel = Bool,
  )
  ```
  This allows users to run their test using a desired kernel version.
  `install_headers` and `install_devel` will install kernel headers if
  the values are set to true, as some tests need them (bcc on kernels <5.2)
  The difference in the packages themselves is that kernel-devel installs
  the entire source tree for the kernel in /usr/src, while kernel-headers
  provides only the header files for userspace tools that need them to compile.

- Currently, `run_as_user` and `hostname` are not supported by VM tests.
  There are plans to add support for these options (see T62319183).

- The `boot` option doesn't affect either of vm.{cpp,python}_unittest
  as there is no way to run VM tests in non-booted mode.

- vm.{cpp,python}_unittest has default values for both the image layer
  as well as the kernel to use. These defaults are `default_vm_image` and
  `default_vm_kernel` respectively. This means that when defining a vm.{cpp,python}_unittest,
  it is not necessary to provide either `kernel_opts` or `layer`.
"""

load("@bazel_skylib//lib:types.bzl", "types")
load(":image.bzl", "image")
load(":image_unittest_helpers.bzl", helpers = "image_unittest_helpers")
load(":oss_shim.bzl", "buck_sh_test", "cpp_unittest", "default_vm_image", "get_visibility", "kernel_artifact", "python_binary", "python_unittest")

_RULE_TO_TEST_TYPE = {
    cpp_unittest: "gtest",
    python_unittest: "pyunit",
}

def _vm_opts(
        kernel,
        install_headers = False,
        install_devel = False):
    return struct(
        kernel = kernel,
        install_headers = install_headers,
        install_devel = install_devel,
    )

_default_vm_opts = _vm_opts(kernel_artifact.default_kernel)

def _tags(unittest_rule, unittest_kwargs):
    """
    Convert top-level 'tags' kwargs into separate tag sets for the outer and
    inner test rules.

    'tags' provided by a user are always applied to the outer test, so they
    control the behavior of TestPilot or to add information for 'buck query'.
    """
    outer_tags = unittest_kwargs.get("tags", []) + ["vmtest"]

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
        **unittest_kwargs):
    inner_test_name = helpers.hidden_test_name(name)
    unittest_rule(
        name = inner_test_name,
        tags = inner_tags,
        visibility = [],
        **unittest_kwargs
    )
    return inner_test_name

def _run_vmtest_name(name):
    return "{}=runvm".format(name)

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
        ncpus):
    if ncpus == 2:
        fail("ncpus=2 will cause kernel panic: https://fburl.com/md27i5k8")

    visibility = get_visibility(visibility, name)

    if not env:
        env = {}

    python_binary(
        name = _run_vmtest_name(name),
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
            kernel.vm,
        ],
    )

    # building a buck_sh_test with a specific type lets us trick TestPilot into
    # thinking that it is running a unit test of the specific type directly,
    # when in reality vmtest.par will transparently run the binary in a VM
    # buck will write the given type into the external_runner_spec.json that is
    # given to TestPilot
    buck_sh_test(
        name = name,
        # We will manually extract any environment variables set and format
        # them into `--setenv NAME=VALUE`. THese are passed during the call to
        # vmtest which will forward them inside the vm for the inner test.
        args = [
            "--setenv={}={}".format(
                var_name,
                var_value,
            )
            for var_name, var_value in env.items()
        ] + ["--ncpus={}".format(ncpus)],
        labels = tags,
        test = ":" + _run_vmtest_name(name),
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
        layer = default_vm_image.layer,
        kernel_opts = None,
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
        ncpus = 1,
        **unittest_kwargs):
    inner_tags, outer_tags = _tags(unittest_rule, unittest_kwargs)
    unittest_kwargs.pop("tags", None)

    inner_test = _inner_test(
        name,
        unittest_rule,
        inner_tags,
        antlir_rule = "user-internal",
        **unittest_kwargs
    )

    if not kernel_opts:
        kernel_opts = _default_vm_opts

    # install the inner test binary at a well-known location in the guest image
    image.layer(
        name = "{}_test_layer".format(name),
        features = [
            image.install_buck_runnable(":" + inner_test, "/test"),
        ],
        antlir_rule = "user-internal",
    )
    image.package(
        name = "{}=test.btrfs".format(name),
        layer = ":{}_test_layer".format(name),
    )

    features = []

    # some tests need kernel headers (bcc on kernels <5.2), so provide an
    # opt-in way to install them in the test image, instead of forcing it
    # for every test, which leads back to the per-kernel image insanity
    if kernel_opts.install_headers or kernel_opts.install_devel:
        features.append(
            image.rpms_install([kernel_opts.kernel.headers, kernel_opts.kernel.devel]),
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
            antlir_rule = "user-internal",
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
        kernel = kernel_opts.kernel,
        ncpus = ncpus,
        seed_device = seed_device,
        tags = outer_tags,
        unittest_rule = unittest_rule,
        user_specified_deps = user_specified_deps,
        visibility = visibility,
    )

def _vm_cpp_unittest(
        name,
        layer = default_vm_image.layer,
        kernel_opts = None,
        deps = (),
        ncpus = 1,
        **cpp_unittest_kwargs):
    _vm_unittest(
        name,
        cpp_unittest,
        layer = layer,
        kernel_opts = kernel_opts,
        deps = deps,
        user_specified_deps = deps,
        ncpus = ncpus,
        **cpp_unittest_kwargs
    )

def _vm_python_unittest(
        name,
        layer = default_vm_image.layer,
        kernel_opts = None,
        ncpus = 1,
        **python_unittest_kwargs):
    # Short circuit the target graph by attaching user_specified_deps to the outer
    # test layer.
    user_specified_deps = []
    user_specified_deps += python_unittest_kwargs.get("deps", [])
    user_specified_deps += python_unittest_kwargs.get("runtime_deps", [])
    resources = python_unittest_kwargs.get("resources", [])
    if types.is_dict(resources):
        resources = list(resources.keys())
    user_specified_deps += resources

    _vm_unittest(
        name,
        python_unittest,
        layer = layer,
        kernel_opts = kernel_opts,
        user_specified_deps = user_specified_deps,
        # unittest pars must be xars so that native libs work inside the vm without
        # linking / mounting trickery
        par_style = "xar",
        ncpus = ncpus,
        **python_unittest_kwargs
    )

def _vm_multi_kernel_unittest(
        name,
        vm_unittest,
        kernels,
        headers = False,
        devel = False,
        **vm_unittest_kwargs):
    for suffix, kernel in kernels.items():
        vm_unittest(
            name = "-".join([name, suffix]),
            kernel_opts = _vm_opts(
                kernel = kernel,
                install_headers = headers,
                install_devel = devel,
            ),
            **vm_unittest_kwargs
        )

def _vm_multi_kernel_cpp_unittest(
        name,
        **vm_cpp_unittest_kwargs):
    _vm_multi_kernel_unittest(
        name,
        _vm_cpp_unittest,
        **vm_cpp_unittest_kwargs
    )

def _vm_multi_kernel_python_unittest(
        name,
        **vm_python_unittest_kwargs):
    _vm_multi_kernel_unittest(
        name,
        _vm_python_unittest,
        **vm_python_unittest_kwargs
    )

vm = struct(
    default = struct(
        kernel = kernel_artifact.default_kernel,
        layer = default_vm_image.layer,
        opts = _vm_opts(kernel_artifact.default_kernel),
    ),
    cpp_unittest = _vm_cpp_unittest,
    multi_kernel = struct(
        cpp_unittest = _vm_multi_kernel_cpp_unittest,
        python_unittest = _vm_multi_kernel_python_unittest,
    ),
    python_unittest = _vm_python_unittest,
    opts = _vm_opts,
)
