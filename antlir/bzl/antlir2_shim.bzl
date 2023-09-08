# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "export_file")
load(":build_defs.bzl", "is_buck2", "python_unittest")
load(":flavor.shape.bzl", "flavor_t")
load(":target_helpers.bzl", "antlir_dep")
load(":types.bzl", "types")

types.lint_noop(flavor_t)

_DEFAULT_ENABLED_PACKAGES = [
    "antlir/bzl/linux",
    "antlir/linux",
    "bot_generated/antlir/fbpkg/db",
    "kernel/kernels",
    "metalos",
    "netos",
    "os_foundation",
    "sandcastle/images",
    "tupperware/image/features",
    "tupperware/image/base",
    "tupperware/image/rpmbuild",
    "tupperware/image/webfoundation",
]

_DEFAULT_DISABLED_PACKAGES = [
    # fbcode/antlir just has way too many weird and broken images, don't even
    # try to make antlir2 images for them when the goal is to delete it all soon
    # enough anyway
    "antlir",
    "sandcastle/images/runtime",
    "sandcastle/images/worker",
    "tupperware/cm/antlir/tests",
    "tupperware/cm/tests",
    "tupperware/image/slimos",
]

def _antlir2_setting_buck1(x):
    return x

# @lint-ignore BUCKLINT
antlir2_setting = native.enum(
    "yes",  # enable antlir2 shadow
    "no",  # disable antlir2 without a recorded reason
    "chef",  # disable antlir2 because chef is natively supported
    "debuginfo",  # antlir2 does not yet support this TODO(T153698233)
    "extract",  # native antlir2 feature
    "rpmbuild",  # antlir2 does not allow rpm installation during a genrule
    # user wants to explicitly alias a built layer instead of downloading it from fbpkg
    "skip-fbpkg-indirection",
    # tests are natively supported and do not require the same antlir1 indirections
    "test",
) if is_buck2() else _antlir2_setting_buck1

def _antlir2_or_default(antlir2: antlir2_setting | None, default: bool) -> bool:
    if antlir2 != None:
        return antlir2 == antlir2_setting("yes")
    else:
        return default

_FLAVOR_T = types.optional(types.union(
    types.str,
    types.shape(flavor_t),
))

types.lint_noop(_FLAVOR_T)

def _should_make_parallel(
        antlir2: str | bool | None,
        *,
        flavor: _FLAVOR_T = None,
        disabled_packages: list[str] = _DEFAULT_DISABLED_PACKAGES,
        enabled_packages: list[str] = _DEFAULT_ENABLED_PACKAGES) -> bool:
    if types.is_bool(antlir2):
        antlir2 = antlir2_setting("yes" if antlir2 else "no")
    else:
        antlir2 = antlir2_setting(antlir2) if antlir2 else None

    # TODO(vmagro): make True the default
    result = False

    # Find the "closest" match so that _DEFAULT_{ENABLED,DISABLED}_PACKAGES can
    # peacefully co-exist
    distance = 999999999
    for pkg in enabled_packages:
        if native.package_name() == pkg or native.package_name().startswith(pkg + "/"):
            this_distance = len(native.package_name()[len(pkg):].split("/"))
            if this_distance < distance:
                result = _antlir2_or_default(antlir2, True)
                distance = this_distance
    for pkg in disabled_packages:
        if native.package_name() == pkg or native.package_name().startswith(pkg + "/"):
            this_distance = len(native.package_name()[len(pkg):].split("/"))
            if this_distance < distance:
                result = _antlir2_or_default(antlir2, False)
                distance = this_distance

    return result

def _fake_buck1_layer(name):
    # export a target of the same name to make td happy
    _fake_buck1_target(name = name + ".antlir2")
    _fake_buck1_target(name = name + ".antlir2.antlir2")
    _fake_buck1_target(name = name + ".antlir2--features")

def _fake_buck1_test(name, test = None):
    _fake_buck1_target(name = name + ".antlir2")
    if test == "python":
        python_unittest(
            name = name + ".antlir2_image_test_inner",
            antlir_rule = "user-facing",
        )
    else:
        _fake_buck1_target(name = name + ".antlir2_image_test_inner")

def _fake_buck1_target(name):
    # export a target of the same name to make td happy
    export_file(
        name = name,
        src = antlir_dep(":empty"),
        antlir_rule = "user-internal",
    )

antlir2_enabled_packages = [
    "metalos/imaging_initrd",
    "metalos/initrd",
]

antlir2_shim = struct(
    fake_buck1_layer = _fake_buck1_layer,
    fake_buck1_feature = _fake_buck1_target,
    fake_buck1_target = _fake_buck1_target,
    fake_buck1_test = _fake_buck1_test,
    should_make_parallel_feature = _should_make_parallel,
    should_make_parallel_layer = _should_make_parallel,
    should_make_parallel_test = _should_make_parallel,
    should_make_parallel_package = partial(_should_make_parallel, enabled_packages = antlir2_enabled_packages),
)
