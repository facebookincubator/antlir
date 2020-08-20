#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from fs_image.nspawn_in_subvol.tests.base import NspawnTestBase


class SlowSudoTestCase(NspawnTestBase):
    """
    A regression test for a bug which caused `sudo` to wait for systemd for
    a long time (order of minutes), causing tests to time out.

    The set-up for our specific bug are as follows:
      - The payload (starting with `timeout` below) is not in the cgroup of
        the container's `systemd` -- which was the case when our `booted.py`
        simply `nsenter`ed into the `systemd`'s container.
      - The host has `cgroup2` mounted with `nsdelegate`, preventing the
        container's `systemd` from moving processes around between certain
        cgroups (see `man 7 cgroups`).
      - The PAM configuration for `sudo` includes `pam_systemd.so`, which
        tries to start sudo's child process in a user session of the ambient
        systemd.
      - At the time that we run `sudo`:
          - The container's `systemd` does not already have a working user
            session for the target user of the `sudo` -- this means that
            `pam_systemd.so` will have to wait for `systemd` to create one.
          - The D-Bus socket of the container's `systemd` is already set up.
            Otherwise, `pam_systemd.so` would conclude that it's on a
            non-`systemd` OS, and fail open.

    If all of the prerequisites are met, then `systemd` (at least as of 245
    or below) will fail to create the user session scope, because its
    attempt to move the `sudo` process into a new cgroup will fail due to
    the `nsdelegate` setting (note that the error, is a confusing `ENOENT`):

        session-1748.scope: Failed to add PIDs to scope's control group:
        No such file or directory

    This, in turn, will cause `sudo` to wait forever (or at least until a
    really long timeout) for a response on the D-Bus socket.
    """

    def test_slow_sudo(self):
        self._nspawn_in(
            (__package__, "build-appliance"),
            [
                "--user=root",
                "--boot",
                "--",
                "timeout",
                "90",
                "/bin/sh",
                "-uexc",
                """
                until test -e /run/dbus/system_bus_socket ; do
                    sleep 0.1
                done
                sudo echo ohai
                """,
            ],
        )
