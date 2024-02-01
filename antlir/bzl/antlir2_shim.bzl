# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:build_defs.bzl", "alias", "export_file")
load(":build_defs.bzl", "is_buck2", "python_unittest")
load(":flavor.shape.bzl", "flavor_t")
load(":target_helpers.bzl", "antlir_dep")
load(":types.bzl", "types")

types.lint_noop(flavor_t)

def _should_shadow(*args, **kwargs) -> bool:
    return False

def _should_upgrade() -> bool:
    if not is_buck2():
        return False
    return True

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
