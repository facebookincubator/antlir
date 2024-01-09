# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "alias", "export_file")
load(":antlir2_migration.bzl?v2_only", "antlir2_migration")
load(":build_defs.bzl", "is_buck2", "python_unittest")
load(":flavor.shape.bzl", "flavor_t")
load(":structs.bzl", "structs")
load(":target_helpers.bzl", "antlir_dep")
load(":types.bzl", "types")

types.lint_noop(flavor_t)

def _antlir2_setting_buck1(x):
    return x

# @lint-ignore BUCKLINT
antlir2_setting = native.enum(
    "yes",  # enable antlir2 shadow
    "no",  # disable antlir2 without a recorded reason
    "chef",  # disable antlir2 because chef is natively supported
    "debuginfo",  # antlir2 natively supports this
    "extract",  # native antlir2 feature
    "rpmbuild",  # antlir2 does not allow rpm installation during a genrule
    # user wants to explicitly alias a built layer instead of downloading it from fbpkg
    "skip-fbpkg-indirection",
    # tests are natively supported and do not require the same antlir1 indirections
    "test",
) if is_buck2() else _antlir2_setting_buck1

def _should_shadow(antlir2: str | bool | None) -> bool:
    if not is_buck2():
        return False

    if antlir2 == None and is_buck2():
        package_mode = antlir2_migration.get_mode()
        return package_mode == antlir2_migration.mode_t("shadow")

    # otherwise, PACKAGE value takes a back-seat to the explicit 'antlir2' flag

    if types.is_bool(antlir2):
        antlir2 = antlir2_setting("yes" if antlir2 else "no")
    else:
        antlir2 = antlir2_setting(antlir2) if antlir2 else None

    return antlir2 == antlir2_setting("yes")

def _should_upgrade() -> bool:
    if not is_buck2():
        return False
    package_mode = antlir2_migration.get_mode()
    return package_mode == antlir2_migration.mode_t("upgrade")

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

def _upgrade_or_shadow(
        *,
        name: str,
        antlir2: str | bool | None,
        fn,
        fake_buck1: struct,
        **kwargs) -> str | None:
    if _should_upgrade():
        fn(name = name, **kwargs)
        alias(
            name = name + ".antlir2",
            actual = ":" + name,
            antlir_rule = "user-internal",
        )
        return "upgrade"
    if _should_shadow(antlir2 = antlir2):
        fn(name = name + ".antlir2", **kwargs)
        if not is_buck2():
            fake = structs.to_dict(fake_buck1)
            fake_fn = fake.pop("fn")
            fake_fn(**fake)
    return None

def _upgrade_or_shadow_feature(
        *,
        name: str,
        antlir2: str | bool | None,
        fn,
        **kwargs) -> str | None:
    if _should_upgrade():
        fn(name = name, **kwargs)
        return "upgrade"
    if _should_shadow(antlir2 = antlir2):
        fn(name = name, **kwargs)
    return None

def _getattr_buck2(val, attr):
    if is_buck2():
        return getattr(val, attr)
    else:
        return None

antlir2_shim = struct(
    fake_buck1_layer = _fake_buck1_layer,
    fake_buck1_feature = _fake_buck1_target,
    fake_buck1_target = _fake_buck1_target,
    fake_buck1_test = _fake_buck1_test,
    should_upgrade_layer = _should_upgrade,
    should_upgrade_feature = _should_upgrade,
    should_shadow_layer = _should_shadow,
    should_shadow_feature = _should_shadow,
    upgrade_or_shadow_layer = _upgrade_or_shadow,
    upgrade_or_shadow_feature = _upgrade_or_shadow_feature,
    upgrade_or_shadow_test = _upgrade_or_shadow,
    upgrade_or_shadow_package = _upgrade_or_shadow,
    getattr_buck2 = _getattr_buck2,
)
