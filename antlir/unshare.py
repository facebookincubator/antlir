#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import enum
import getpass
import logging
import os
import subprocess
import sys
from typing import AnyStr, Iterable, List, Union


# This module is never __main__, so the module name should be sane
# pyre-fixme[5]: Global expression must be annotated.
log = logging.getLogger(__name__)


@enum.unique
class Namespace(enum.Enum):
    # Private: the values happen to be `unshare` and `nsenter` flags.
    #
    # Uncomment other namespaces as needed, but be sure to add tests.  Most
    # will Just Work (TM).
    #
    # NB: User namespaces aren't supported both because we have kernels that
    # turn them off, and because this would require extra work for non-root
    # execution.
    #
    # CGROUP = '--cgroup'
    # IPC = '--ipc'
    MOUNT = "--mount"
    NETWORK = "--net"
    PID = "--pid"
    # UTS = '--uts'


class Unshare:
    """
    This context manager lets a non-root process run commands inside a set
    of Linux namespaces, either as root, or as the original user.

    This creates a dummy keepalive process, which owns the specified set of
    namespaces.  The keepalive exits whenever this context exits, triggering
    namespace cleanup.  The keepalive also exits whenever its stdin pipe is
    closed, which means it ought not outlive the parent.

    Our actual implementation immediately passes ownership of the namespaces
    to the current process, so for most namespaces, this class will continue
    working as intended even if the keepalive process is killed.  This, of
    course, is not true for PID namespaces -- those can only work while the
    keepalive process exists (since it is the `init` of the namespace).


    ## Gotchas of the current PID namespace implementation

    The namespace's `init` is `cat`. This causes two defects:
      - It lacks normal signal handling for `init`. This is "OK", since the
        current unprivileged process cannot signal `sudo` children anyway.
      - It does not wait for children reparented to it. The resulting
        zombies could be a cosmetic problem if a substantial number of
        children are started via `nsenter_*` commands.

    Besides the bad `init`, we also fail to mount a new `/proc` inside the
    PID namespace, so programs like `ps` will be broken under `nsenter_*`
    commands.

    The ONLY intention of this broken PID namespace implementation is to
    make the calling process less likely to leak `sudo` its descendants, and
    it succeeds at that.  But it is not a proper container system.


    ## Design notes / future work

    Future: An `Unshare` object could easily support running `unshare` after
    `nsenter`ing into another `Unshare` -- a form of nesting.

    Future: The `nsenter_*` calls could support entering
    a subset of the unshared namespaces.

    NB: The `run_as_user` functionality exists to annotate whether the end
    logic requires privileges or not.  The specific implementation of
    setting to the original effective UID/GID after `nsenter` definitely
    does not restore the SAME authentication context that the parent has --
    it's just an approximation (e.g. supplementary groups are dropped).
    """

    # pyre-fixme[4]: Attribute must be annotated.
    _NS_TO_PROC_FILENAME = {
        # Namespace.CGROUP: 'cgroup',
        # Namespace.IPC: 'ipc',
        Namespace.MOUNT: "mnt",
        Namespace.NETWORK: "net",
        Namespace.PID: "pid",
        # Namespace.UTS: 'uts',
    }
    assert set(Namespace) == set(_NS_TO_PROC_FILENAME.keys())

    # pyre-fixme[3]: Return type must be annotated.
    def __init__(self, namespaces: Iterable[Namespace]):
        # pyre-fixme[4]: Attribute must be annotated.
        self._namespaces = frozenset(namespaces)
        # pyre-fixme[4]: Attribute must be annotated.
        self._keepalive_proc = None
        # Instead of using `nsenter --target KEEPALIVE_PID`, we will
        # actually open all the namespace FDs in the current process.  This
        # eliminates an important failure mode at runtime.
        # pyre-fixme[4]: Attribute must be annotated.
        self._namespace_to_file = None
        # pyre-fixme[4]: Attribute must be annotated.
        self._root_fd = None

    def __enter__(self) -> "Unshare":
        assert not self._keepalive_proc, "Unshare is not reentrant"
        # This `cat` keeps alive our mount + PID namespaces. It exits when
        # its input pipe closes, which is much cleaner than having to `sudo
        # kill $PID_OF_SUDO` and hoping for the best.  This FD-based
        # keepalive ensures that our mounts or related "sudo" processes
        # don't outlive the main process, and don't spuriously leak.  We
        # could also use PR_SET_PDEATHSIG, but I'm not sure it adds much.
        #
        # NBL Since python 3.4, the pipe has O_CLOEXEC, so ownership of the
        # Unshare is truly confined to the creating process.
        self._keepalive_proc = subprocess.Popen(
            [
                # NB: `--mount-proc` is NOT passed, even if `Namespace.PID` is
                # specified.  We use this to populate `self._namespace_to_file`
                # below, but if `--mount-proc` is necessary, this could be
                # fixed.  Note also that `--pid` with `--mount-proc` without an
                # explicit `--mount` should probably always imply `--propagation
                # unchaged`, or any mount commands wrapped by `nsenter_*`
                # methods would NOT be visible to the current process.
                "sudo",
                "unshare",
                "--fork",
                *(ns.value for ns in self._namespaces),
                # Without switching to the parent's euid, the parent would
                # not be able to open the keepalive's namespaces below.
                "nsenter",
                "--setuid",
                f"{os.geteuid()}",
                "--setgid",
                f"{os.getegid()}",
                "bash",
                "-euc",
                "exec 3< /proc/self/status ; grep ^NSpid <&3 ; exec cat 1>&2",
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
        )
        try:
            # `sudo` keeps stdout open, so we have to read just 1 line.
            # pyre-fixme[16]: Optional type has no attribute `readline`.
            nspid_out = self._keepalive_proc.stdout.readline().split()
            self._keepalive_proc.stdout.close()  # Nothing should write here
            # We do NOT pass `--mount-proc` to `unshare`, so we can inspect
            # the `/proc` from the parent's PID namespace to efficiently
            # find the PID of the keepalive `cat` process.  If we HAD to
            # pass `--mount-proc`, a way to fix this might be to make a
            # second mount of the parent's proc in a private location on the
            # filesystem...  or for the parent to search its own `/proc`.
            # Another way would be to have the child PID `sendmsg` us the
            # relevant FDs through a Unix domain socket.  Perhaps the
            # least-bad way would be for us to add a `mount_proc` kwarg,
            # which would explicitly implement the right behavior (depending
            # on whether or not Namespace.MOUNT is specified) AFTER
            # `self._namespace_to_file` is already populated.
            rc = self._keepalive_proc.poll()
            assert not rc, f"keepalive process exited unexpectedly {rc}"
            assert nspid_out, "failed to collect namespace pid"
            assert nspid_out[0] == b"NSpid:", nspid_out
            if Namespace.PID in self._namespaces:
                assert nspid_out[-1] == b"1", nspid_out
                ns_pid = int(nspid_out[-2])
            else:
                ns_pid = int(nspid_out[-1])
                assert ns_pid != 1, nspid_out
            # `nsenter -m` (by design) escapes chroots, whereas we want to
            # keep the child's execution environment as similar as possible
            # to ours.  So we need to set the root.  Unfortunately, due to
            # what's likely a kernel bug, most ways of passing `--root` to
            # `nsenter` will result in an empty `/proc/mounts` in the child.
            # The only thing that works is to re-use the root FD that
            # belongs to the `unshare`d `cat` -- and we must `open` it, so
            # as not to depend on the `cat` staying alive.
            self._root_fd = os.open(f"/proc/{ns_pid}/root", 0)
            self._namespace_to_file = {}
            for ns in self._namespaces:
                self._namespace_to_file[ns] = open(
                    f"/proc/{ns_pid}/ns/{self._NS_TO_PROC_FILENAME[ns]}", "rb"
                )
            # At this point, the current process has FDs for all the
            # unshared namespaces, so the `nsenter_*` methods need not rely
            # on the accessibility of `/proc/KEEPALIVE_PID`.  Of course, it
            # is still impossible to enter a PID namespace after the
            # keepalive has exited.
            #
            # I believe that unless `self._nas_args` contains `Namespace.PID`,
            # we COULD now safely reap the keepalive process.  But it's not
            # that expensive to keep it around, so I leave the logic simple.
        except BaseException:
            self.__exit__(*sys.exc_info())
            raise
        return self

    # This context does not suppress exception
    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def __exit__(self, exc_type, exc_val, exc_tb):
        if self._namespace_to_file is not None:
            # We need to close the files to let the namespaces be destroyed
            for f in self._namespace_to_file.values():
                try:
                    f.close()
                # Covering this realistically seems really hard, since this
                # should never fail. Manual test:
                #
                #   >>> log = logging.getLogger('moo')
                #   >>> f = open('/proc/self/ns/mnt', 'rb')
                #   >>> try:
                #   ...     1/0
                #   ... except:
                #   ...     log.exception(f'Closing namespace file {f.name}')
                #   ...
                #   Closing namespace file /proc/self/ns/mnt
                #   Traceback (most recent call last):
                #     File "<stdin>", line 2, in <module>
                #     ZeroDivisionError: division by zero
                except BaseException:  # pragma: no cover
                    log.exception(f"Closing namespace file {f.name}")
            self._namespace_to_file = None

        try:
            if self._root_fd:
                os.close(self._root_fd)
        # Same coverage story as above for the `f.close()`
        except BaseException:  # pragma: no cover
            log.exception(f"Closing root directory FD {self._root_fd}")
        self._root_fd = None

        if self._keepalive_proc:
            try:
                self._keepalive_proc.stdin.close()  # "Normally" won't fail
                if self._keepalive_proc.wait() != 0:  # prag
                    log.warning(
                        "Unshare keepalive exited with {}".format(
                            self._keepalive_proc.returncode
                        )
                    )
            finally:
                self._keepalive_proc = None
            # By this point, the namespaces should be getting torn down.

    def _nsenter_args(self) -> List[str]:
        if self._namespace_to_file is None:
            raise RuntimeError("Must nsenter from inside an Unshare context")
        # The namespace FDs are O_CLOEXEC, so they are only accessible
        # through the current process.
        cur_pid = os.getpid()
        return [
            # If we happen to be in a chroot, we don't want the `nsenter`ed
            # process to end running with the original root, which is what
            # `nsenter` would do without `--root`.
            #
            # Unlike the `--root` shenanigan documented in `__enter__`, I am
            # not aware of any downsides to using `--wd=.`, and it does have
            # the advantage of presenting the target process with a
            # predictable working directory (compare: `unshare -m` changes
            # the working directory to `/`, while `unshare -p` does not).
            f"--root=/proc/{cur_pid}/fd/{self._root_fd}",
            "--wd=.",
            *(
                f"{ns.value}=/proc/{cur_pid}/fd/{f.fileno()}"
                for ns, f in self._namespace_to_file.items()
            ),
        ]

    def nsenter_without_sudo(self, *cmd: AnyStr) -> List[Union[str, bytes]]:
        return ["nsenter", *self._nsenter_args(), *cmd]

    def nsenter_as_root(self, *cmd: AnyStr) -> List[Union[str, bytes]]:
        return ["sudo", "nsenter", *self._nsenter_args(), *cmd]

    def nsenter_as_user(self, *cmd: AnyStr) -> List[Union[str, bytes]]:
        return [
            "sudo",
            "env",
            "--",
            f"USER={getpass.getuser()}",
            f"LOGNAME={getpass.getuser()}",
            "nsenter",
            *self._nsenter_args(),
            # Pretend we did not `sudo` (see the note in the class docstring)
            "--setuid",
            str(os.geteuid()),
            "--setgid",
            str(os.getegid()),
            *cmd,
        ]


# pyre-fixme[2]: Parameter must be annotated.
def nsenter_as_root(unshare, *cmd: AnyStr) -> List[Union[str, bytes]]:
    "Unshare.nsenter_as_root that also handles unshare=None"
    if unshare is None:
        return ["sudo", *cmd]
    return unshare.nsenter_as_root(*cmd)


# pyre-fixme[2]: Parameter must be annotated.
def nsenter_as_user(unshare, *cmd: AnyStr) -> List[Union[str, bytes]]:
    "Unshare.nsenter_as_user that also handles unshare=None"
    if unshare is None:
        return [*cmd]
    return unshare.nsenter_as_user(*cmd)
