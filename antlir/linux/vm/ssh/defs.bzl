# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl/image/feature:defs.bzl", "feature")

def _test_only_login():
    """
    Configure ssh login for root using the generic VM public key.  This is used
    only for testing and should never be installed into a production image.
    """
    return [
        feature.ensure_subdirs_exist(
            "/root",
            ".ssh",
            user = "root",
            group = "root",
            mode = "u+rx",
        ),
        feature.install(
            "//antlir/linux/vm/ssh:pubkey",
            "/root/.ssh/authorized_keys",
            user = "root",
            group = "root",
            mode = "u+r",
        ),
    ]

def _hostkey_setup():
    # This section customizes the generation of ssh host keys to reduce the startup
    # time by ~2 full seconds by:
    #   - Generating only one host key and
    #   - Using /run/sshd to store the host key

    return [
        feature.install("//antlir/linux/vm/ssh:sshd.tmpfiles.conf", "/usr/lib/tmpfiles.d/sshd.tmpfiles.conf"),
        # sshd-keygen.service doesn't exist on centos9
        feature.remove("/usr/lib/systemd/system/sshd-keygen.service", must_exist = False),
        systemd.install_unit("//antlir/linux/vm/ssh:sshd-keygen.service"),
        systemd.enable_unit("sshd-keygen.service", "core-services.target"),
        # Install a drop-in that updates the cmd line to include the
        # custom hostkey location.
        systemd.install_dropin("//antlir/linux/vm/ssh:sshd-hostkey.conf", "sshd.service"),
    ]

ssh = struct(
    test_only_login = _test_only_login,
    hostkey_setup = _hostkey_setup,
)
