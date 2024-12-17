# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def sysroot_dep(
        *,
        name: str,
        link: str | None = None,
        visibility: list[str] = ["PUBLIC"]):
    """
    A cxx_library target that links against a library that exists in the
    sysroot, but does not have any pkg-config definition, just a linker flag.
    """
    native.prebuilt_cxx_library(
        name = name,
        exported_linker_flags = [
            "-l" + (link or name),
        ],
        visibility = visibility,
    )
