# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKLINT

load("//antlir/bzl:oss_shim.bzl", "is_buck2")

ItemInfo = native.provider(
    fields = [
        "items",
    ],
) if is_buck2() else None

RpmInfo = native.provider(
    fields = [
        "action",
        "flavors",
    ],
) if is_buck2() else None

FlavorInfo = native.provider(
    fields = [
        "flavors",
    ],
) if is_buck2() else None
