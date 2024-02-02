# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":build_defs.bzl", "is_buck2")
load(":flavor.shape.bzl", "flavor_t")
load(":types.bzl", "types")

types.lint_noop(flavor_t)

def _should_shadow(*args, **kwargs) -> bool:
    return False

def _should_upgrade() -> bool:
    if not is_buck2():
        return False
    return True

def _upgrade_or_shadow(
        *,
        name: str,
        antlir2: str | bool | None,
        fn,
        **kwargs) -> str | None:
    if _should_upgrade():
        fn(name = name, **kwargs)
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
