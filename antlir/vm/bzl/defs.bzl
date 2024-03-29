# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Antlir1 vmtests are no longer supported. Use Antlir2 vmtest instead. Check
antlir2/antlir2_vm/bzl/defs.bzl.
"""

vm = struct(
    # This nested structure is for looking up the default set of artifacts
    # used for this subsystem.
    artifacts = struct(
        rootfs = struct(
            layer = "fbcode//metalos/vm/os:rootfs",
        ),
    ),
)
