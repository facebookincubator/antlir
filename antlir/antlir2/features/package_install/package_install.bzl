# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features/apt:apt.bzl", "apt_install", "apt_remove")
load("//antlir/antlir2/features/rpm:rpm.bzl", "rpms_install", "rpms_remove")

def package_install(*, subjects: list[str]):
    """
    Install packages by name, automatically selecting the correct package
    manager (apt or dnf) based on the OS configuration.

    Elements in `subjects` are package names like `"bash"` or `"systemd"`.
    """
    return select({
        "//antlir/antlir2/os/package_manager:package_manager[apt]": apt_install(packages = subjects),
        "//antlir/antlir2/os/package_manager:package_manager[dnf5]": rpms_install(subjects = subjects),
        "//antlir/antlir2/os/package_manager:package_manager[dnf]": rpms_install(subjects = subjects),
        "DEFAULT": select_fail("cannot install packages without a package manager"),
    })

def package_remove(*, subjects: list[str]):
    """
    Remove packages by name, automatically selecting the correct package
    manager (apt or dnf) based on the OS configuration.

    Elements in `subjects` are package names. If a package is not installed,
    this feature will fail.
    """
    return select({
        "//antlir/antlir2/os/package_manager:package_manager[apt]": apt_remove(packages = subjects),
        "//antlir/antlir2/os/package_manager:package_manager[dnf5]": rpms_remove(rpms = subjects),
        "//antlir/antlir2/os/package_manager:package_manager[dnf]": rpms_remove(rpms = subjects),
        "DEFAULT": select_fail("cannot remove packages without a package manager"),
    })
