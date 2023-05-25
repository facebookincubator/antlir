# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "export_file")
load(":build_defs.bzl", "is_buck2")
load(":flavor.shape.bzl", "flavor_t")
load(":flavor_impl.bzl", "flavor_to_struct")
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
    "tupperware/image/features",
    "tupperware/image/base/impl/features",
    "tupperware/image/rpmbuild",
]

_DEFAULT_DISABLED_PACKAGES = [
    # fbcode/antlir just has way too many weird and broken images, don't even
    # try to make antlir2 images for them when the goal is to delete it all soon
    # enough anyway
    "antlir",
    "tupperware/cm/antlir/tests",
    "tupperware/cm/tests",
    "tupperware/image/slimos",
    # This has some really huge chef recipes that are currently giving us
    # trouble. Exclude until we can get taht resolved soon
    "os_foundation/images/impl",
]

def _antlir2_setting_buck1(x):
    return x

# @lint-ignore BUCKLINT
antlir2_setting = native.enum(
    "yes",  # enable antlir2 shadow
    "no",  # disable antlir2 without a recorded reason
    "centos7",  # disable antlir2 because of centos7
    "chef",  # disable antlir2 because chef is natively supported
    "debuginfo",  # antlir2 does not yet support this TODO(T153698233)
    "rpmbuild",  # antlir2 does not allow rpm installation during a genrule
) if is_buck2() else _antlir2_setting_buck1

def _antlir2_or_default(antlir2: [
    antlir2_setting.type,
    None,
], default: bool.type) -> bool.type:
    if antlir2 != None:
        return antlir2 == antlir2_setting("yes")
    else:
        return default

def _should_make_parallel(
        antlir2: [
            str.type,
            bool.type,
            None,
        ],
        *,
        flavor: types.optional(types.union(
            types.str,
            types.shape(flavor_t),
        )) = None) -> bool.type:
    if flavor and flavor_to_struct(flavor).name == "centos7":
        return False

    if types.is_bool(antlir2):
        antlir2 = antlir2_setting("yes" if antlir2 else "no")
    else:
        antlir2 = antlir2_setting(antlir2) if antlir2 else None

    # TODO(vmagro): make True the default
    result = False

    # Find the "closest" match so that _DEFAULT_{ENABLED,DISABLED}_PACKAGES can
    # peacefully co-exist
    distance = 999999999
    for pkg in _DEFAULT_ENABLED_PACKAGES:
        if native.package_name() == pkg or native.package_name().startswith(pkg + "/"):
            this_distance = len(native.package_name()[len(pkg):].split("/"))
            if this_distance < distance:
                result = _antlir2_or_default(antlir2, True)
                distance = this_distance
    for pkg in _DEFAULT_DISABLED_PACKAGES:
        if native.package_name() == pkg or native.package_name().startswith(pkg + "/"):
            this_distance = len(native.package_name()[len(pkg):].split("/"))
            if this_distance < distance:
                result = _antlir2_or_default(antlir2, False)
                distance = this_distance

    return result

def _fake_buck1_layer(name):
    # export a target of the same name to make td happy
    export_file(
        name = name + ".antlir2",
        src = antlir_dep(":empty"),
        antlir_rule = "user-internal",
    )
    export_file(
        name = name + ".antlir2.antlir2",
        src = antlir_dep(":empty"),
        antlir_rule = "user-internal",
    )
    export_file(
        name = name + ".antlir2--features",
        src = antlir_dep(":empty"),
        antlir_rule = "user-internal",
    )

def _fake_buck1_target(name):
    # export a target of the same name to make td happy
    export_file(
        name = name,
        src = antlir_dep(":empty"),
        antlir_rule = "user-internal",
    )

antlir2_shim = struct(
    fake_buck1_layer = _fake_buck1_layer,
    fake_buck1_feature = _fake_buck1_target,
    fake_buck1_target = _fake_buck1_target,
    should_make_parallel_feature = _should_make_parallel,
    should_make_parallel_layer = _should_make_parallel,
)
