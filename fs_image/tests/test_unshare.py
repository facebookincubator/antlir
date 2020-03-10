#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import re
import signal
import subprocess
import tempfile
import time
import unittest

from typing import Iterable
from unittest import mock

from unshare import Namespace, nsenter_as_root, nsenter_as_user, Unshare

# `user` omitted for reasons described in Unshare's docblock
_NS_FILES = ['cgroup', 'ipc', 'mnt', 'net', 'pid', 'uts']


class UnshareTestCase(unittest.TestCase):

    def test_nsenter_wrappers(self):
        self.assertEqual(('a', 'b'), nsenter_as_user(None, 'a', 'b'))
        self.assertEqual(('sudo', 'c', 'd'), nsenter_as_root(None, 'c', 'd'))

    def _popen_sleep_forever(self, unshare: Unshare):
        # We need the ready signal to know when we've actually executed the
        # payload -- otherwise, we might try to interact with it while we're
        # still at `nsenter`.
        proc = subprocess.Popen(nsenter_as_user(
            unshare, 'bash', '-uec', 'echo ready $$ ; exec sleep infinity',
        ), stdout=subprocess.PIPE)

        # Wait for the child to start
        ready_and_pid = proc.stdout.readline().split(b' ')
        self.assertEqual(b'ready', ready_and_pid[0])

        proc.stdout.close()  # `sudo` keeps stdout open, but will not write.
        # Returning the PID lets us clean up the `sleep infinity` when it is
        # not inside a PID namespace.
        return proc, int(ready_and_pid[1])

    def _check_ns_diff(self, unshare: Unshare, ns_diff: Iterable[str]):
        list_ns_cmd = [
            'readlink', *(f'/proc/self/ns/{name}' for name in _NS_FILES),
        ]
        in_ns, out_ns = [
            dict(
                ns_ino.split(':') for ns_ino in
                    subprocess.check_output(cmd).decode().strip().split('\n')
            ) for cmd in [
                list_ns_cmd, nsenter_as_root(unshare, *list_ns_cmd),
            ]
        ]
        for ns in ns_diff:
            self.assertNotEqual(in_ns.pop(ns), out_ns.pop(ns), ns)
        self.assertEqual(in_ns, out_ns)

    def _kill_keepalive(self, unshare: Unshare):
        # We can kill the inner keepalive `cat` since it runs w/ our UID
        # Since it's an `init` of a PID namespace, we must use SIGKILL.
        cat_pid = int(re.match(
            '^/proc/([0-9]+)/ns/',
            next(iter(unshare._namespace_to_file.values())).name,
        ).group(1))
        print('Sending SIGKILL to', cat_pid)
        os.kill(cat_pid, signal.SIGKILL)

    def test_pid_namespace(self):
        with Unshare([Namespace.PID]) as unshare:
            proc, _ = self._popen_sleep_forever(unshare)
            # Check that "as user" works.
            for arg, expected in (('-u', os.geteuid()), ('-g', os.getegid())):
                actual = int(subprocess.check_output(nsenter_as_user(
                    unshare, 'id', arg
                )))
                self.assertEqual(expected, actual)
            time.sleep(2)  # Leave some time for `sleep` to exit erroneously
            self.assertEqual(None, proc.poll())  # Sleeps forever

            self._check_ns_diff(unshare, {'pid'})

        self.assertEqual(-signal.SIGKILL, proc.poll())  # Reaped by PID NS

    def test_pid_namespace_dead_keepalive(self):
        with Unshare([Namespace.PID]) as unshare:
            self._check_ns_diff(unshare, {'pid'})

            good_echo = nsenter_as_user(unshare, 'echo')
            subprocess.check_call(good_echo)  # Will fail once the NS is dead

            proc, _ = self._popen_sleep_forever(unshare)
            time.sleep(2)  # Leave some time for `sleep` to exit erroneously
            self.assertEqual(None, proc.poll())  # Sleeps forever

            self._kill_keepalive(unshare)

            self.assertEqual(-signal.SIGKILL, proc.wait())  # The NS is dead

            # The `echo` command that worked above no longer works.
            with self.assertRaises(subprocess.CalledProcessError):
                subprocess.check_call(good_echo)

    def test_context_enter_error(self):
        'Exercise triggering __exit__ when __enter__ raises'
        unshare = Unshare([Namespace.MOUNT])  # This does not fail
        # Give bad arguments to the inner `sudo` to make the keepalive fail
        # quickly without outputting the inner PID.
        with mock.patch('os.geteuid', side_effect='NOT-A-REAL-USER-ID'), \
                self.assertRaises(IndexError):  # nspid_out[0] fails
            with unshare:
                raise AssertionError   # Guarantees __enter__ was what failed
        # The Unshare was left in a clean-ish state, which strongly suggests
        # that __exit__ ran, given that __enter__ immediately assigns to
        # `self._keepalive_proc`, and that did run (CalledProcessError).
        self.assertEqual(None, unshare._keepalive_proc)
        self.assertEqual(None, unshare._namespace_to_file)

    def test_no_namespaces(self):
        '''
        A silly test that shows that unsharing nothing still works -- which
        is useful to distinguish self._namespace_to_file {} vs None.  That
        said, people should just use nsenter_as_*(None, ...) instead.
        '''
        with Unshare([]) as unshare:
            self._check_ns_diff(unshare, {})

    def test_multiple_namespaces(self):
        'Just a smoke test for multiple namespaces being entered at once'
        with Unshare([Namespace.PID, Namespace.MOUNT]) as unshare:
            self._check_ns_diff(unshare, {'mnt', 'pid'})

    def test_mount_namespace(self):
        try:
            sleep_pid = None
            with tempfile.TemporaryDirectory() as mnt_src, \
                    tempfile.TemporaryDirectory() as mnt_dest1, \
                    tempfile.TemporaryDirectory() as mnt_dest2:
                with open(os.path.join(mnt_src, 'cypa'), 'w') as outfile:
                    outfile.write('kvoh')

                def check_mnt_dest(mnt_dest: str):
                    cypa = os.path.join(mnt_dest, 'cypa')
                    # The outer NS cannot see the mount
                    self.assertFalse(os.path.exists(cypa))
                    # But we can read it from inside the namespace
                    self.assertEqual(b'kvoh', subprocess.check_output(
                        nsenter_as_user(unshare, 'cat', cypa),
                    ))

                with Unshare([Namespace.MOUNT]) as unshare:
                    # Without a PID namespace, this will outlive the
                    # __exit__ -- in fact, this process would leak but for
                    # our `finally`.
                    proc, sleep_pid = self._popen_sleep_forever(unshare)

                    subprocess.check_call(nsenter_as_root(
                        unshare, 'mount', mnt_src, mnt_dest1, '-o', 'bind',
                    ))
                    check_mnt_dest(mnt_dest1)

                    # Mount namespaces remain usable after the keepalive dies
                    self._kill_keepalive(unshare)

                    # We can make a second mount inside the namespace
                    subprocess.check_call(nsenter_as_root(
                        unshare, 'mount', mnt_src, mnt_dest2, '-o', 'bind',
                    ))
                    check_mnt_dest(mnt_dest2)
                    check_mnt_dest(mnt_dest1)  # The old mount is still good

                # Outside the context, nsenter cannot work. There's no way
                # to test the mounts are gone since we don't have any handle
                # by which to access them.  That's the point.
                with self.assertRaisesRegex(
                    RuntimeError, 'Must nsenter from inside an Unshare',
                ):
                    check_mnt_dest(mnt_dest1)

            time.sleep(2)  # Give some time for `sleep` to exit erroneously
            self.assertIs(None, proc.poll())  # Processes leak
        finally:
            # Ensure we don't leak the `sleep infinity` -- since it was
            # started via `sudo`, `subprocess` cannot kill it automatically.
            if sleep_pid:
                if proc.poll() is None:
                    os.kill(sleep_pid, signal.SIGTERM)
                proc.wait()
