# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")

def _autologin():
    """
    Install image features to enable auto login of root on the ttyS0
    serial console.
    """

    # Enable auto-login of root user on ttyS0
    return [
        image.install(
            "//antlir/linux/vm/console:autologin-root.conf",
            "/usr/lib/systemd/system/serial-getty@ttyS0.service.d/autologin-root.conf",
        ),
        image.ensure_subdirs_exist(
            "/usr/lib/systemd/system",
            "serial-getty@ttyS0.service.d",
            mode = "a+rx,u+w",
        ),
    ]

console = struct(
    autologin = _autologin,
)
