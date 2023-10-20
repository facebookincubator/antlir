# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def set_default_os_for_package(*, default_os: str):
    write_package_value(
        "antlir2.default_os",
        default_os,
        overwrite = True,
    )

def get_default_os_for_package() -> str:
    return read_package_value("antlir2.default_os")

def all_images_in_package_use_default_os(yes: bool = True):
    write_package_value(
        "antlir2.all_images_in_package_use_default_os",
        yes,
        overwrite = True,
    )

def should_all_images_in_package_use_default_os() -> bool:
    return read_package_value(
        "antlir2.all_images_in_package_use_default_os",
    ) or False
