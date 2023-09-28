# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Lists pre-configured VM targets that any test can refer to based on their needs.
# Refer to actual target for their detailed configuration.
PRECONFIGURED_VM = {
    "disk-boot": "//antlir/antlir2/antlir2_vm:default-disk-boot",
    "nondisk-boot": "//antlir/antlir2/antlir2_vm:default-nondisk-boot",
    "nvme-disk-boot": "//antlir/antlir2/antlir2_vm:default-nvme-disk-boot",
}

def get_vm(name: str = "nondisk-boot") -> str:
    if name not in PRECONFIGURED_VM:
        fail("{} not listed in pre-configured VMs".format(name))
    return PRECONFIGURED_VM[name]
