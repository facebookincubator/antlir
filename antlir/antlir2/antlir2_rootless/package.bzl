# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

_KEY = "antlir2.rootless"

def _write_package_value(*args, **kwargs):
    write_package_value = getattr(native, "write_package_value", None)
    if write_package_value != None:
        write_package_value(*args, **kwargs)

def _read_package_value(*args, **kwargs):
    read_package_value = getattr(native, "read_package_value", None)
    if read_package_value != None:
        return read_package_value(*args, **kwargs)
    return None

def antlir2_rootless(*, rootless: bool):
    _write_package_value(_KEY, rootless, overwrite = True)

def get_antlir2_rootless() -> bool:
    return _read_package_value(_KEY) or bool(int(native.read_config("antlir2", "rootless", 0)))

def antlir2_rootless_config_set() -> bool:
    return _read_package_value(_KEY) != None or native.read_config("antlir2", "rootless", None) != None
