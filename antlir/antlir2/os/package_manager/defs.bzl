# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

_PACKAGE_MANAGER_SELECT = select({
    "//antlir/antlir2/os/package_manager:package_manager[apt]": "apt",
    "//antlir/antlir2/os/package_manager:package_manager[dnf5]": "dnf5",
    "//antlir/antlir2/os/package_manager:package_manager[dnf]": "dnf",
    "//antlir/antlir2/os/package_manager:package_manager[none]": "none",
})

package_manager_selected_attr = attrs.default_only(
    attrs.string(
        default = _PACKAGE_MANAGER_SELECT,
    )
)
