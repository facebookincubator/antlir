# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

###
### IMPORTANT: This is NOT a Buck binary, it runs using either:
###   - The system Python2 (for CentOS7 et al)
###   - The system Python3 (CentOS8, Fedora, etc)
### The precise Python is selected by `fake_pty_wrapper.py`.
###
import ctypes
import fcntl
import os
import signal
import struct
import sys
import termios

_prctl = ctypes.CDLL("libc.so.6").prctl
_PR_SET_PDEATHSIG = 1

ptm, pts = os.openpty()
# rows, columns, xpixel, ypixel
s = struct.pack("HHHH", 20, 1000, 0, 0)
fcntl.ioctl(pts, termios.TIOCSWINSZ, s)

pid = os.fork()
if pid == 0:
    # Make sure the child is killed when we exit
    _prctl(_PR_SET_PDEATHSIG, signal.SIGKILL)
    os.dup2(pts, 1)
    os.dup2(pts, 2)
    os.execvp(sys.argv[1], sys.argv[1:])

os.close(pts)

# If the child gets interrupted, so do we. Ignore, we'll exit with the child.
signal.signal(signal.SIGINT, signal.SIG_IGN)

while True:
    try:
        chunk = os.read(ptm, 4096)  # Raises OSError when the child exits
        os.write(1, chunk)
    except OSError:
        break  # Exit, killing the child (it's already dead on OSError)

_, status = os.waitpid(pid, 0)
exit(status)
