# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Lists pre-configured VM targets that any test can refer to based on their needs.
# Refer to actual target for their detailed configuration.

DEFAULT = "//antlir/antlir2/antlir2_vm:default-nondisk-boot"

DEFAULT_DISK_BOOT = "//antlir/antlir2/antlir2_vm:default"
DEFAULT_NONDISK_BOOT = "//antlir/antlir2/antlir2_vm:default-nondisk-boot"
