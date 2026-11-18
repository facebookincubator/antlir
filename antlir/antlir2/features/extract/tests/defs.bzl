# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")

def library_path(name: str):
    return select({
        "//antlir/antlir2/os/family:family[debian]": arch_select(
            aarch64 = "/usr/lib/aarch64-linux-gnu/" + name,
            x86_64 = "/usr/lib/x86_64-linux-gnu/" + name,
        ),
        "DEFAULT": "/usr/lib64/" + name,
    })
