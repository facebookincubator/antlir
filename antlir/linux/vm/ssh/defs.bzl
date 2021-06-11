# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")

def _test_only_login():
    """
    Configure ssh login for root using the generic VM public key.  This is used
    only for testing and should never be installed into a production image.
    """
    return [
        image.ensure_subdirs_exist(
            "/root",
            ".ssh",
            user = "root",
            group = "root",
            mode = "u+rx",
        ),
        image.install(
            "//antlir/linux/vm/ssh:pubkey",
            "/root/.ssh/authorized_keys",
            user = "root",
            group = "root",
            mode = "u+r",
        ),
    ]

ssh = struct(
    test_only_login = _test_only_login,
)
