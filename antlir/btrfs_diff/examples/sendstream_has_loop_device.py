#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Usage:

    python3 -m btrfs_diff.examples.sendstream_has_loop_device < sendstream ||
        echo No loop or loop-control

Reads a send-stream from stdin, prints to stdout the major & minor of the
first loop or loop-control device found, and returns 0.

Returns 2 if no loops exist, 1 on usage or data errors.
"""
import os
import sys

from antlir.btrfs_diff.parse_send_stream import parse_send_stream
from antlir.btrfs_diff.send_stream import SendStreamItems


def main(argv):
    if len(argv) != 1:
        print(__doc__, file=sys.stderr)
        return 1
    for item in parse_send_stream(sys.stdin.buffer):
        if isinstance(item, SendStreamItems.mknod) and (
            os.major(item.dev) == 7 or item.dev == os.makedev(10, 237)
        ):
            # Not printing the path here since it'd be `o123-78-456` or some
            # similarly meaningless temporary emitted by `btrfs send`.
            #
            # To get the path, we would instead apply the send-stream to a
            # `Subvolume`, and use `.inodes()` to look for loops.  The
            # downside of that style of check is that it requires us to
            # process a sequence of send-streams in dependency order (or
            # we'd hit dependency errors), whereas just scanning the
            # send-stream is cheap.
            #
            # There are a couple of other approaches to getting the path:
            #  -  Roll some special logic for resolving what names
            #     send-stream temporaries ultimately map to.  Probably not
            #     worth it.
            #  -  Add the capability to `Subvolume` to apply items even when
            #     the dependency is not there, and instead to record some
            #     kind of placeholder / dependency object in the tree.  The
            #     semantics would take some thought to get right, but the
            #     upside is significant, since we would then be able to
            #     handle filesystem diffs almost as easily as full
            #     filesystems.
            print(os.major(item.dev), os.minor(item.dev))
            return 0
    return 2  # Python would return 1 on raised parse exceptions :)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
