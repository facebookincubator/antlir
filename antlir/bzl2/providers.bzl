# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKLINT

load("//antlir/bzl2:use_buck2_macros.bzl", "use_buck2_macros")

ItemInfo = native.provider(
    fields = [
        "items",
    ],
) if use_buck2_macros() else None

RpmInfo = native.provider(
    fields = [
        "action",
        "flavors",
    ],
) if use_buck2_macros() else None

FlavorInfo = native.provider(
    fields = [
        "flavors",
    ],
) if use_buck2_macros() else None
