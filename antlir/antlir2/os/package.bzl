# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

_DEFAULT_OS_KEY = "antlir2.default_os"

def write_package_value(*args, **kwargs):
    write_package_value = getattr(native, "write_package_value", None)
    if write_package_value != None:
        write_package_value(*args, **kwargs)

def read_package_value(*args, **kwargs):
    read_package_value = getattr(native, "read_package_value", None)
    if read_package_value != None:
        return read_package_value(*args, **kwargs)
    return None

def set_default_os_for_package(*, default_os: str):
    write_package_value(_DEFAULT_OS_KEY, default_os, overwrite = True)

def get_default_os_for_package() -> str:
    return read_package_value(_DEFAULT_OS_KEY) or "centos9"

def all_images_in_package_use_default_os(
        yes: bool = True):  # @unused
    # Must still exist until the `redundant_default_os` codemod is complete and
    # removed all callsites
    pass
