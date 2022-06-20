load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:oss_shim.bzl", "third_party", antlir_rust_binary = "rust_binary", antlir_rust_library = "rust_library", antlir_rust_unittest = "rust_unittest")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/vm/bzl:defs.bzl", "vm")
load(":metalos_tests.shape.bzl", "container_unittest_opts_t", "unittest_opts_t")

_unittest_flavors = ("plain", "container", "vm")

# Nicer rust wrapping logic with a few nice-to-have features.
# 1. Easier access to third-party dependencies.
#      Because rust crates are not namespaced, any dependency without a
#      tell-tale buck ':' will be assumed to be a third-party dependency
# 2. A variety of unittest flavors.
#      MetalOS has a wide range of rust code that demands different testing
#      environments. A good rule of thumb for which test environments to use is:
#      - logic: plain
#      - requires root: container
#      - requires simulated hardware interactions: vm
#
#      Tests can be written in the regular rust convention, but with additional
#      attributes to add to test functions.
#      Test functions can be annotated with one of the following:
#        - #[test]: regular logic test
#        - #[containertest]: only run in containers
#        - #[vmtest]: only run in virtual machines
#      Tests with an attribute other than the current environment will be
#      skipped.
def _rust_common(
        rule,
        name,
        srcs = (),
        test_srcs = (),
        unittests = True,
        unittest_opts = None,
        deps = (),
        rustc_flags = None,
        test_deps = (),
        test_env = None,
        tests = (),
        features = (),
        vm_opts = (),
        __metalctl_only_allow_unused_deps = False,
        **kwargs):
    if types.is_bool(unittests):
        unittests = ["plain"] if unittests else []
    for flavor in unittests:
        if flavor not in _unittest_flavors:
            fail(
                "'{}' is not a supported rust unittest flavor. Options are {}"
                    .format(flavor, ", ".join(_unittest_flavors)),
            )
    deps = [_normalize_rust_dep(d) for d in deps]
    test_deps = list(test_deps)
    if ("container" in unittests) or ("vm" in unittests):
        test_deps += ["//metalos/metalos_macros:metalos_macros", "tokio"]
    test_deps = [_normalize_rust_dep(d) for d in test_deps]

    rustc_flags = rustc_flags or []
    # @oss-disable: rustc_flags.append("--cfg=facebook") 
    if not __metalctl_only_allow_unused_deps:
        rustc_flags.append("--forbid=unused_crate_dependencies")
    kwargs["rustc_flags"] = rustc_flags

    tests = list(tests)
    tests += [":" + name + _unittest_suffix(flavor) for flavor in unittests]
    if kwargs.get("proc_macro", False):
        unittests = []
        tests = []

    crate = kwargs.pop("crate", name.replace("-", "_"))
    if len(srcs) == 1 and not kwargs.get("crate_root", None):
        kwargs["crate_root"] = srcs[0]

    rule(
        name = name,
        srcs = srcs,
        crate = crate,
        unittests = False,
        deps = deps,
        tests = tests,
        features = features,
        **kwargs
    )

    if not unittest_opts:
        unittest_opts = unittest_opts_t(container = shape.new(container_unittest_opts_t))

    features = list(features)
    test_kwargs = dict(kwargs)
    test_kwargs.pop("link_style", None)
    test_kwargs.pop("allocator", None)
    test_kwargs.pop("linker_flags", None)
    test_kwargs.pop("proc_macro", None)
    if test_env:
        test_kwargs["env"] = test_env

    srcs = list(srcs)
    test_srcs = list(test_srcs) if test_srcs else []

    if "plain" in unittests:
        antlir_rust_unittest(
            name = name + _unittest_suffix("plain"),
            srcs = srcs + test_srcs,
            crate = crate,
            deps = deps + test_deps,
            features = features + ["metalos_plain_test"],
            **test_kwargs
        )
    if "container" in unittests:
        image.rust_unittest(
            name = name + _unittest_suffix("container"),
            srcs = srcs + test_srcs,
            crate = crate,
            deps = deps + test_deps,
            features = features + ["metalos_container_test"],
            layer = unittest_opts.container.layer,
            run_as_user = "root",
            boot = unittest_opts.container.boot,
            **test_kwargs
        )
    if "vm" in unittests:
        vm.rust_unittest(
            name = name + _unittest_suffix("vm"),
            srcs = srcs + test_srcs,
            crate = crate,
            deps = deps + test_deps,
            features = features + ["metalos_vm_test"],
            vm_opts = vm_opts,
            **test_kwargs
        )

def _unittest_suffix(flavor):
    if flavor == "plain":
        return "-unittest"
    return "-" + flavor + "-unittest"

def _normalize_rust_dep(dep):
    if ":" in dep:
        return dep
    return third_party.library(dep, platform = "rust")

def rust_binary(name, **kwargs):
    _rust_common(antlir_rust_binary, name, **kwargs)

def rust_library(name, **kwargs):
    _rust_common(antlir_rust_library, name, **kwargs)

def rust_unittest(name, srcs, deps, **kwargs):
    deps = [_normalize_rust_dep(d) for d in deps]
    antlir_rust_unittest(
        name = name,
        srcs = srcs,
        deps = deps,
        **kwargs
    )

def default_test_layer():
    return container_unittest_opts_t().layer
