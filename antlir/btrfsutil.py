# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import errno
import functools
import multiprocessing
import multiprocessing.connection
import os
import subprocess
import sys
from typing import Optional

import btrfsutil
import btrfsutil as _raw_btrfsutil
from antlir.common import get_logger
from antlir.fs_utils import Path
from antlir.unshare import Unshare


log = get_logger()


def _sudo_retry(f, args, kwargs, in_namespace: Optional[Unshare] = None):
    func = f.__name__
    with Path.resource("antlir", "btrfsutil-bin", exe=True) as btrfsutil_bin:
        (args_pipe_r, args_pipe_w) = multiprocessing.Pipe(duplex=False)
        (return_pipe_r, return_pipe_w) = multiprocessing.Pipe(duplex=False)
        args_pipe_w.send(args)
        args_pipe_w.send(kwargs)
        cmd = [
            btrfsutil_bin,
            func,
        ]
        if in_namespace is not None:
            cmd = in_namespace.nsenter_as_root(*cmd)
        else:
            cmd = ["sudo"] + cmd
        try:
            subprocess.run(
                cmd,
                stdin=args_pipe_r,
                stdout=return_pipe_w,
                check=True,
                env={"PYTHONDONTWRITEBYTECODE": "1"},
            )
            return return_pipe_r.recv()
        except subprocess.CalledProcessError:
            inner_ex = return_pipe_r.recv()
            raise inner_ex from None


__ALWAYS_SUDO_CALLS = {
    # so that created subvols are correctly owned by root
    "create_subvolume",
    # this can report ENOENT even in cases of permissions problems, and we know
    # it requires root anyway
    "delete_subvolume",
}


# Wrap a named function with an automatic sudo retry. This does not accept a
# first-class function object so that the raw functions can be mocked in unit
# tests
def __with_sudo_retry(name):
    f = getattr(_raw_btrfsutil, name)

    @functools.wraps(f)
    def wrapper(*args, in_namespace: Optional[Unshare] = None, **kwargs):
        f = getattr(_raw_btrfsutil, name)

        if name in __ALWAYS_SUDO_CALLS or in_namespace is not None:
            return _sudo_retry(f, args, kwargs, in_namespace=in_namespace)

        try:
            res = f(*args, **kwargs)
            return res
        except btrfsutil.BtrfsUtilError as be:
            if be.errno == errno.EPERM:
                try:
                    log.debug(
                        f"btrfsutil.{f.__name__}({repr(args), {repr(kwargs)}})"
                        " got EPERM, retrying via sudo"
                    )
                    return _sudo_retry(f, args, kwargs)
                except Exception as e:
                    # replace the original PermissionError with the internal
                    # exception encountered while retrying with sudo
                    raise e from None
            else:
                raise be

    return wrapper


__WRAP_BLOCKLIST = [_raw_btrfsutil.BtrfsUtilError, _raw_btrfsutil.SubvolumeInfo]

# Wrap all btrfsutil functions with an automatic retry on permissions error.  If
# a permission error is encountered, this script will be re-run under sudo and
# try to call the function again. When this code is called as root, this is very
# fast, and when called as a non-privileged user, is no worse than using the
# btrfs cli, but comes with nicer properties (for example, recursive subvol
# delete is natively supported)
for name in dir(_raw_btrfsutil):
    item = getattr(_raw_btrfsutil, name)
    if item in __WRAP_BLOCKLIST:
        globals()[name] = item
        continue
    if callable(item):
        globals()[name] = __with_sudo_retry(name)


def main() -> None:  # pragma: no cover
    parser = argparse.ArgumentParser()
    parser.add_argument("func")
    args = parser.parse_args()

    args_pipe = multiprocessing.connection.Connection(sys.stdin.fileno())
    return_pipe = multiprocessing.connection.Connection(sys.stdout.fileno())

    if os.geteuid() != 0:
        return_pipe.send(RuntimeError("btrfsutil binary must be called as root"))
        sys.exit(1)

    func = args.func
    args = args_pipe.recv()
    kwargs = args_pipe.recv()

    if func == "unittest_fail":
        return_pipe.send(RuntimeError("failing for unittest coverage"))
        sys.exit(1)

    try:
        # we are already running as root, use the raw btrfsutil function, not
        # our retrying wrapper
        res = getattr(btrfsutil, func)(*args, **kwargs)
        return_pipe.send(res)
    except BaseException as ex:
        return_pipe.send(ex)
        print(ex, file=sys.stderr)
        sys.exit(1)
    finally:
        return_pipe.close()
        sys.stdout.flush()


# this is covered by the integration tests, but is in a separate binary
# invocation so doesn't get counted
if __name__ == "__main__":
    main()  # pragma: no cover
