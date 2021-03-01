# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")

def _host():
    """
    Configure the Guest -> Host networking inside the guest vm.
    """
    return [
        image.install("//antlir/linux/vm/network:host0.link", "/usr/lib/systemd/network/10-host0.link"),
        image.install("//antlir/linux/vm/network:host0.network", "/usr/lib/systemd/network/10-host0.network"),
    ]

network = struct(
    host = _host,
)
