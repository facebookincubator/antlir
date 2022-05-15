#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Usage:

  python3 -m btrfs_diff.examples.sendstreams_to_json_subvolumes \
      sendstream1 sendtream2 ... | jq -C . | less -R

You will want to refer to `_repr_fields` in `inode.py` to understand this
tool's notation for filesystem metadata.

A few things to try:

 - Pass `/dev/fd/0` for stdin,
 - Run this on the "demo send-streams" from our tests via:

     alias demo_sendstream='python3 -m btrfs_diff.tests.gold_demo_sendstreams'

     python3 -m btrfs_diff.examples.sendstreams_to_json_subvolumes \
       --show-only mutate_ops \
       <(demo_sendstream create_ops) <(demo_sendstream mutate_ops) |
           jq -C . | less -R

    Omit `--show-only` to see the cross-snapshot clone structure.

    NB While `btrfs receive --dump` has bugs (see `parse_dump.py`), you may
    find this helpful: `demo_sendstream create_ops | btrfs receive --dump`.

  - Compare our JSON output via `diff`, since its keys are already sorted.
    For unsorted JSON, use `diff <(jq -S . a.json) <(jq -S . b.json)`.

"""
# NB This was cribbed from `test_sendstream_to_subvolume_set_integration.py`
# to encourage interactive play with send-streams.
import argparse
import json
import sys

from ..freeze import freeze
from ..inode import InodeOwner
from ..inode_utils import (
    erase_mode_and_owner,
    erase_selinux_xattr,
    erase_utimes_in_range,
    SELinuxXAttrStats,
)
from ..parse_send_stream import parse_send_stream
from ..rendered_tree import emit_non_unique_traversal_ids
from ..subvolume_set import SubvolumeSet, SubvolumeSetMutator


def main(argv):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--no-check-complete",
        action="store_true",
        help="By default, we assert that the send-streams do not leave any "
        "inodes with underspecified metadata.",
    )
    parser.add_argument(
        "--no-erase-default-mode-and-owner",
        action="store_true",
        help="By default, we hide from the output `o0:0`, plus `m755` for "
        "directories, and `m644` for other inode types.",
    )
    parser.add_argument(
        "--erase-utimes-in-range",
        metavar=("START_SEC", "START_NSEC", "END_SEC", "END_NSEC"),
        type=int,
        nargs=4,
        default=(0, 0, 2**64, 10**9),
        help="By default, we will hide all file timestamps. Pass four -1s "
        "to show all timestamps. Or pass a range specified as 2 pairs "
        "of second + nanosecond integers. This is intended to hide "
        "timestamps implicitly assigned during a build, since those "
        "may not be so meaningful to the reader.",
    )
    parser.add_argument(
        "--erase-selinux-xattr",
        metavar="WHICH_VALUE",
        type=str,
        default="MOST_COMMON",
        # I am not worried about collisions with actual values since SELinux
        # contexts typically have colons, but my sentinels don't.
        help="Erases all SELinux xattrs with the specified value. Choose from "
        '"MOST_COMMON" (the default), "NONE", or an actual value. '
        "The goal is to highlight the non-default, interesting xattrs. "
        "The sentinels are CASE-SENSITIVE.",
    )
    parser.add_argument(
        "--show-only",
        type=str,
        action="append",
        # Question: Should we fix the fact that the clones won't be shown
        # between selected subvolumes?  It's quite easy, just `deepcopy` the
        # `SubvolumeSet` and delete the ones we don't want.  On the one
        # hand, the lack of clone-spam may be a feature.  On the other hand,
        # it is not great that you cannot tell which clone links are shown
        # (or not) by just looking at the JSON output.
        help="If you have a long chain of snapshots, the cloned extent "
        "printout may be overwhelming. In this case, you can select "
        "a few subvolumes to display individually by repeating this "
        "option. WARNING: this will not show cloned extents between "
        "the selected subvolumes. Note that the subvolumes are "
        "identified by name, with the following disambiguator added "
        'if necessary: "@minimally-unambuguous-uuid-prefix". If in '
        "doubt, first look at the output without `--show-only`.",
    )
    parser.add_argument(
        "sendstream",
        type=argparse.FileType("br"),
        nargs="+",
        help="A file containing the output of `btrfs send`. Note that "
        "send-stream order matters, since we will try to apply them "
        "to our in-memory filesystem from left to right.",
    )
    args = parser.parse_args(argv[1:])

    subvols = SubvolumeSet.new()
    for sendstream_in in args.sendstream:
        parsed = parse_send_stream(sendstream_in)
        mutator = SubvolumeSetMutator.new(subvols, next(parsed))
        for i in parsed:
            mutator.apply_item(i)

    # Check that our send-streams completely specified the subvolumes.
    if not args.no_check_complete:
        for ino in freeze(subvols).inodes():
            ino.assert_valid_and_complete()

    # Render the demo subvolumes after stripping all the predictable
    # metadata to make our "expected" view of the filesystem shorter.
    selinux_stats = SELinuxXAttrStats(subvols.inodes())
    for ino in subvols.inodes():
        if not args.no_erase_default_mode_and_owner:
            erase_mode_and_owner(
                ino,
                owner=InodeOwner(uid=0, gid=0),
                file_mode=0o644,
                dir_mode=0o755,
            )
        erase_utimes_in_range(
            ino,
            start=tuple(args.erase_utimes_in_range[:2]),
            end=tuple(args.erase_utimes_in_range[2:]),
        )
        if args.erase_selinux_xattr != "NONE":
            erase_selinux_xattr(
                ino,
                (
                    selinux_stats.most_common()
                    if args.erase_selinux_xattr == "MOST_COMMON"
                    else args.erase_selinux_xattr
                ),
            )

    if args.show_only:
        result = {}
        # This hides cross-subvolume clone annotations, see `--show-only`.
        for which_subvol in args.show_only:
            subvol = subvols.get_by_rendered_id(which_subvol)
            if subvol is None:
                raise RuntimeError(
                    f"Unknown subvol {which_subvol}, try without --show-only"
                )
            result[which_subvol] = emit_non_unique_traversal_ids(
                freeze(subvol).render()
            )
    else:
        result = freeze(subvols).map(
            lambda sv: emit_non_unique_traversal_ids(sv.render())
        )
    # Future: is there a `pprint`-style compact & pretty JSON output?
    print(json.dumps(result, sort_keys=True, indent=2))


if __name__ == "__main__":
    main(sys.argv)
