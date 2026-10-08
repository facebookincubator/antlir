# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""PACKAGE-level default for vmtest prewarming.

Put this in a `PACKAGE` file to opt a whole directory tree into restoring a
pre-booted VM instead of booting one per test:

    load("@antlir//antlir/antlir2/antlir2_vm/bzl:package.bzl", "vm_prewarm")

    vm_prewarm(enabled = True)

An explicit `prewarm = ...` on an individual test always wins. With neither, the
default is False.
"""

_KEY = "antlir2_vm.prewarm"

def _write_package_value(*args, **kwargs):
    write_package_value = getattr(native, "write_package_value", None)
    if write_package_value != None:
        write_package_value(*args, **kwargs)

def _read_package_value(*args, **kwargs):
    read_package_value = getattr(native, "read_package_value", None)
    if read_package_value != None:
        return read_package_value(*args, **kwargs)
    return None

def vm_prewarm(*, enabled: bool):
    """Default every vmtest under this package to (not) prewarm."""
    _write_package_value(_KEY, enabled, overwrite = True)

def get_vm_prewarm_default() -> bool:
    """Resolve the prewarm default for a test that did not specify one."""
    val = _read_package_value(_KEY)
    if val != None:
        return val
    return False
