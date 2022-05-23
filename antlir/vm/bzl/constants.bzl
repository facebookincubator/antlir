# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# MAC address for use in virtual machines.  Each VM is in its own network
# namespace, so this value is constant for all VMs.  Keep this in sync with
# //antlir/vm/tap.py.
VM_GUEST_MAC_ADDRESS = "00:00:00:00:00:01"
