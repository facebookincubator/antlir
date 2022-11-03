#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
import sys
from typing import List, Sequence

from antlir.btrfs_diff.parse_dump import (
    NAME_TO_PARSER_TYPE,
    parse_btrfs_dump,
    unquote_btrfs_progs_path,
)
from antlir.btrfs_diff.send_stream import SendStreamItem, SendStreamItems
from antlir.btrfs_diff.tests.demo_sendstreams import (
    gold_demo_sendstreams,
    make_demo_sendstreams,
)
from antlir.btrfs_diff.tests.demo_sendstreams_expected import (
    get_filtered_and_expected_items,
    parse_demo_sendstreams_btrfs_dump,
)

from antlir.fs_utils import Path
from antlir.tests.common import AntlirTestCase


def _parse_demo_lines_to_list(s: Sequence[bytes]) -> List[SendStreamItem]:
    return list(parse_demo_sendstreams_btrfs_dump(io.BytesIO(b"\n".join(s) + b"\n")))


def _parse_lines_to_list(s: Sequence[bytes]) -> List[SendStreamItem]:
    return list(parse_btrfs_dump(io.BytesIO(b"\n".join(s) + b"\n")))


class ParseBtrfsDumpTestCase(AntlirTestCase):
    def test_unquote(self) -> None:
        self.assertEqual(
            (b"\a\b\x1b\f\n\r\t\v " rb"\XYZ\F\0\O\P"),
            unquote_btrfs_progs_path(
                # Special escapes
                rb"\a\b\e\f\n\r\t\v\ \\"
                # Octal escapes
                + "".join(f"\\{ord(c):o}" for c in "XYZ").encode("ascii")
                # Unrecognized escapes will be left alone
                + rb"\F\0\O\P"
            ),
        )

    def test_ensure_demo_sendstreams_cover_all_operations(self) -> None:
        # Ensure we have implemented all the operations from here:
        # https://github.com/kdave/btrfs-progs/blob/master/send-dump.c#L319
        expected_ops = {
            "chmod",
            "chown",
            "clone",
            "link",
            "mkdir",
            "mkfifo",
            "mkfile",
            "mknod",
            "mksock",
            "remove_xattr",
            "rename",
            "rmdir",
            "set_xattr",
            "snapshot",
            "subvol",
            "symlink",
            "truncate",
            "unlink",
            "update_extent",
            "utimes",
            # Omitted since `--dump` never prints data: 'write',
        }
        self.assertEqual({n.decode() for n in NAME_TO_PARSER_TYPE.keys()}, expected_ops)

        # Now check that `demo_sendstream.py` also exercises those operations.
        stream_dict = make_demo_sendstreams(Path(sys.argv[0]))
        out_lines = [
            *stream_dict["create_ops"]["dump"],
            *stream_dict["mutate_ops"]["dump"],
        ]
        self.assertEqual(
            expected_ops,
            {
                l.split(b" ", 1)[0].decode().replace("write", "update_extent")
                for l in out_lines
                if l
            },
        )
        items = [
            *_parse_demo_lines_to_list(stream_dict["create_ops"]["dump"]),
            *_parse_demo_lines_to_list(stream_dict["mutate_ops"]["dump"]),
        ]
        # We an item per line, and the items cover the expected operations.
        self.assertEqual(len(items), len(out_lines))
        self.assertEqual(
            {getattr(SendStreamItems, op_name) for op_name in expected_ops},
            {i.__class__ for i in items},
        )

    # The reason we want to parse a gold file instead of, as above, running
    # `demo_sendstreams.py` is explained in its top docblock.
    def test_verify_gold_parse(self) -> None:
        stream_dict = gold_demo_sendstreams()
        filtered_items, expected_items = get_filtered_and_expected_items(
            items=_parse_demo_lines_to_list(stream_dict["create_ops"]["dump"])
            + _parse_demo_lines_to_list(stream_dict["mutate_ops"]["dump"]),
            # `--dump` does not show fractional seconds at present.
            # pyre-fixme[6]: For 2nd param expected `float` but got
            #  `Tuple[typing.Any, int]`.
            build_start_time=(
                stream_dict["create_ops"]["build_start_time"][0],
                0,
            ),
            # pyre-fixme[6]: For 3rd param expected `float` but got
            #  `Tuple[typing.Any, int]`.
            build_end_time=(stream_dict["mutate_ops"]["build_end_time"][0], 0),
            dump_mode=True,
        )
        self.assertEqual(filtered_items, expected_items)

    def test_common_errors(self) -> None:
        # Before testing errors, check we can parse the unmodified setup.
        uuid = "01234567-0123-0123-0123-012345678901"
        subvol_line = f"subvol ./s uuid={uuid} transid=12".encode()
        ok_line = b"mkfile ./s/cat\\ and\\ dog"  # Drive-by test of unquoting
        self.assertEqual(
            [
                SendStreamItems.subvol(path=b"s", uuid=uuid.encode(), transid=12),
                SendStreamItems.mkfile(path=b"cat and dog"),
            ],
            _parse_lines_to_list([subvol_line, ok_line]),
        )

        with self.assertRaisesRegex(RuntimeError, "has unexpected format:"):
            _parse_lines_to_list([subvol_line, b" " + ok_line])

        with self.assertRaisesRegex(RuntimeError, "unknown item type b'Xmkfi"):
            _parse_lines_to_list([subvol_line, b"X" + ok_line])

        with self.assertRaisesRegex(RuntimeError, "did not set subvolume"):
            _parse_lines_to_list([ok_line])

        with self.assertRaisesRegex(RuntimeError, "created more than once"):
            _parse_lines_to_list([subvol_line, subvol_line])

        with self.assertRaisesRegex(RuntimeError, "did not start with"):
            _parse_lines_to_list([subvol_line, ok_line.replace(b"/s/", b"/x/")])

        with self.assertRaisesRegex(RuntimeError, "s/t' contains /"):
            _parse_lines_to_list([subvol_line.replace(b"./s", b"./s/t")])

    def test_set_xattr_errors(self) -> None:
        uuid = "01234567-0123-0123-0123-012345678901"

        def make_lines(len_k="len", len_v=7, name_k="name", data_k="data"):
            return [
                f"subvol ./s uuid={uuid} transid=7".encode(),
                f"set_xattr ./s/file {name_k}=MY_ATTR {data_k}=MY_DATA "
                f"{len_k}={len_v}".encode(),
            ]

        # Before breaking it, ensure that `make_lines` actually works
        for data in (b"MY_DATA", b"MY_DATA\0"):
            self.assertEqual(
                [
                    SendStreamItems.subvol(path=b"s", uuid=uuid.encode(), transid=7),
                    SendStreamItems.set_xattr(path=b"file", name=b"MY_ATTR", data=data),
                    # The `--dump` line does NOT show the \0, the parser infers
                    # it.
                ],
                _parse_lines_to_list(make_lines(len_v=len(data))),
            )

        for bad_lines in [
            # Bad field name, non-int value, value inconsistent with data,
            make_lines(len_k="X"),
            make_lines(len_v="x7"),
            make_lines(len_v=9),
            # Swap name & data fields, try a bad one
            make_lines(data_k="name", name_k="data"),
            make_lines(name_k="nom"),
        ]:
            with self.assertRaisesRegex(RuntimeError, "in line details:"):
                _parse_lines_to_list(bad_lines)

    def test_str_uses_unqualified_class_name(self) -> None:
        self.assertEqual(
            "mkfile(path='cat and dog')",
            # pyre-fixme[6]: For 1st param expected `bytes` but got `str`.
            str(SendStreamItems.mkfile(path="cat and dog")),
        )
