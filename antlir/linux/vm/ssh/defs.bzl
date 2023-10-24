# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:defs.bzl?v2_only", antlir2_feature = "feature")
load("//antlir/bzl:systemd.bzl", "systemd")
load("//antlir/bzl/image/feature:defs.bzl", antlir1_feature = "feature")

def _test_only_login(use_antlir2 = False):
    """
    Configure ssh login for root using the generic VM public key.  This is used
    only for testing and should never be installed into a production image.
    """
    return _test_only_login_antlir2() if use_antlir2 else _test_only_login_antlir1()

def _test_only_login_antlir2():
    return [
        antlir2_feature.ensure_subdirs_exist(
            into_dir = "/root",
            subdirs_to_create = ".ssh",
            user = "root",
            group = "root",
            mode = "u+rx",
        ),
        antlir2_feature.install(
            src = "//antlir/linux/vm/ssh:pubkey",
            dst = "/root/.ssh/authorized_keys",
            user = "root",
            group = "root",
            mode = "u+r",
        ),
    ]

def _test_only_login_antlir1():
    return [
        antlir1_feature.ensure_subdirs_exist(
            "/root",
            ".ssh",
            user = "root",
            group = "root",
            mode = "u+rx",
        ),
        antlir1_feature.install(
            "//antlir/linux/vm/ssh:pubkey",
            "/root/.ssh/authorized_keys",
            user = "root",
            group = "root",
            mode = "u+r",
        ),
    ]

def _hostkey_setup(use_antlir2 = False):
    # This section customizes the generation of ssh host keys to reduce the startup
    # time by ~2 full seconds by:
    #   - Generating only one host key and
    #   - Using /run/sshd to store the host key
    return _hostkey_setup_antlir2() if use_antlir2 else _hostkey_setup_antlir1()

def _hostkey_setup_antlir2():
    return [
        antlir2_feature.install(
            src = "//antlir/linux/vm/ssh:sshd.tmpfiles.conf",
            dst = "/usr/lib/tmpfiles.d/sshd.tmpfiles.conf",
        ),
        # sshd-keygen.service doesn't exist on centos9
        antlir2_feature.remove(
            path = "/usr/lib/systemd/system/sshd-keygen.service",
            must_exist = False,
        ),
        # The tmpfiles.d provision.conf file on normal MetalOS image is a slightly modified
        # version of standard systemd tmpfiles.d provision.conf file. It makes it so
        # that /root/.ssh is a symlink to /run/fs/control/run/state/persistent/certs/root_ssh
        # so t hat the generate root ssh key persists reboots and rootfs updates.
        # VM images do not have persistent subvols so here we remove this file.
        # More context in D50266692.
        antlir2_feature.remove(
            path = "/usr/lib/tmpfiles.d/provision.conf",
            must_exist = False,
        ),
        systemd.install_unit(
            "//antlir/linux/vm/ssh:sshd-keygen.service",
            use_antlir2 = True,
        ),
        systemd.enable_unit(
            "sshd-keygen.service",
            "core-services.target",
            use_antlir2 = True,
        ),
        # Install a drop-in that updates the cmd line to include the
        # custom hostkey location.
        systemd.install_dropin(
            "//antlir/linux/vm/ssh:sshd-hostkey.conf",
            "sshd.service",
            use_antlir2 = True,
        ),
    ]

def _hostkey_setup_antlir1():
    return [
        antlir1_feature.install("//antlir/linux/vm/ssh:sshd.tmpfiles.conf", "/usr/lib/tmpfiles.d/sshd.tmpfiles.conf"),
        # sshd-keygen.service doesn't exist on centos9
        antlir1_feature.remove("/usr/lib/systemd/system/sshd-keygen.service", must_exist = False),
        # The tmpfiles.d provision.conf file on normal MetalOS image is a slightly modified
        # version of standard systemd tmpfiles.d provision.conf file. It makes it so
        # that /root/.ssh is a symlink to /run/fs/control/run/state/persistent/certs/root_ssh
        # so t hat the generate root ssh key persists reboots and rootfs updates.
        # VM images do not have persistent subvols so here we remove this file.
        # More context in D50266692.
        antlir1_feature.remove(
            "/usr/lib/tmpfiles.d/provision.conf",
            must_exist = False,
        ),
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
