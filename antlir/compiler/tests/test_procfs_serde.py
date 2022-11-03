#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest

from antlir.compiler.procfs_serde import deserialize_int, deserialize_untyped, serialize

from antlir.subvol_utils import with_temp_subvols
from antlir.tests.subvol_helpers import render_subvol


class TestProcfsSerDe(unittest.TestCase):
    def setUp(self) -> None:
        self._next_dir_idx = 0

    # Gives a differend path for each new calls to `_check_serialize_scalar`.
    # NB: Re-serializing an ever larger subvol is O(N^2) -- if this gets too
    # slow, make this a contextmanager, and clean up old directories.
    def _next_dir(self) -> str:
        self._next_dir_idx += 1
        return str(self._next_dir_idx)

    def _check_serialize_deserialize_idempotence(self, subvol, orig_dir, name_with_ext):
        data = deserialize_untyped(subvol.path(), os.path.join(orig_dir, name_with_ext))
        new_dir = self._next_dir()
        serialize(data, subvol, os.path.join(new_dir, name_with_ext))
        rendered = render_subvol(subvol)[1]
        # Ensure that the metadata of the once-serialized and
        # twice-serialized directories are identical.
        self.assertEqual(rendered[orig_dir], rendered[new_dir])
        # Also compare the file contents match
        subprocess.check_call(
            ["diff", "--recursive", subvol.path(orig_dir), subvol.path(new_dir)]
        )

    def _check_serialize_scalar(self, expected, data, subvol, name_with_ext) -> None:
        outer_dir = self._next_dir()
        path_with_ext = os.path.join(outer_dir, name_with_ext)
        serialize(data, subvol, path_with_ext)
        self._check_serialize_deserialize_idempotence(subvol, outer_dir, name_with_ext)
        self.assertEqual(
            ["(Dir)", {name_with_ext: [f"(File d{len(expected)})"]}],
            render_subvol(subvol)[1][outer_dir],
        )
        with open(subvol.path(path_with_ext), "rb") as f:
            self.assertEqual(expected, f.read())

    def _check_serialize_dict(self, expect_render, data, subvol, name_with_ext) -> None:
        outer_dir = self._next_dir()
        path_with_ext = os.path.join(outer_dir, name_with_ext)
        serialize(data, subvol, path_with_ext)
        self._check_serialize_deserialize_idempotence(subvol, outer_dir, name_with_ext)
        self.assertEqual(["(Dir)", expect_render], render_subvol(subvol)[1][outer_dir])
        # NB: Not checking file contents because the scalar test cover that.

    @with_temp_subvols
    def test_serialize(self, temp_subvols) -> None:
        subvol = temp_subvols.create("x")

        # Valid scalar values
        for expected, actual, name in (
            # No extensions
            (b"\n", "", "la"),
            (b"3\n", 3, "la"),
            (b"7\n", "7", "la"),
            (b"5\n", b"5", "la"),
            (b"31.4\n", 3.14e1, "la"),
            (b"0\n", False, "la"),
            (b"1\n", True, "la"),
            # Paths -- byte & str, plus a path ending in \n
            (b"baa\n", "baa", "x.host_path"),
            (b"mee\n\n", b"mee\n", "x.image_path"),
            # Binary -- adds no trailing newline
            (b"la\nla", b"la\nla", "x.bin"),
        ):
            with self.subTest(expected=expected, actual=actual):
                self._check_serialize_scalar(expected, actual, subvol, name)

        # Errors
        with self.assertRaisesRegex(AssertionError, "add list support if you "):
            serialize([1, 2], subvol, "la")
        with self.assertRaisesRegex(AssertionError, "unhandled type .*Excepti"):
            serialize(Exception(), subvol, "la")
        for name in ("la.host_path", "la.image_path"):
            with self.assertRaisesRegex(AssertionError, "needs str/bytes, got"):
                serialize(1, subvol, name)
        with self.assertRaisesRegex(AssertionError, "needs bytes, got"):
            serialize(1, subvol, "la.bin")
        with self.assertRaisesRegex(AssertionError, "Unsupported extension"):
            serialize(1, subvol, "la.fooext")

        # None produces no output
        outer_dir = self._next_dir()
        serialize(None, subvol, os.path.join(outer_dir, "nothing here"))
        self.assertNotIn(outer_dir, render_subvol(subvol)[1])

        self._check_serialize_dict({"foo": ["(Dir)", {}]}, {}, subvol, "foo")
        self._check_serialize_dict(
            {
                "foobar": [
                    "(Dir)",
                    {
                        "x": ["(File d2)"],
                        "y": ["(File d2)"],
                        "z": ["(Dir)", {"a": ["(File d15)"]}],  # Nested dict
                    },
                ]
            },
            {"x": 5, "y": False, "z": {"a": "a" * 14}},
            subvol,
            "foobar",
        )

    @with_temp_subvols
    def test_deserialize(self, temp_subvols) -> None:
        subvol = temp_subvols.create("y")
        # Writing this test is easier if we don't need to write as root.
        subvol.run_as_root(["chown", f"{os.geteuid()}:{os.getegid()}", subvol.path()])

        # Test type coercion.  Not testing untyped deserialization, since
        # `test_serialize` checks `serialize(deserialize_untyped(x)) == x`.

        with open(subvol.path("valid_int"), "wb") as f:
            f.write(b"37\n")
        self.assertEqual(37, deserialize_int(subvol.path(), "valid_int"))

        # Now check error conditions unrelated to type coercion

        with open(subvol.path("no_newline"), "wb") as f:
            f.write(b"3.14")
        with self.assertRaisesRegex(AssertionError, " a trailing newline,"):
            deserialize_untyped(subvol.path(), "no_newline")

        with open(subvol.path("foo.badext"), "wb") as f:
            f.write(b"\n")
        with self.assertRaisesRegex(AssertionError, "Unsupported extension "):
            deserialize_untyped(subvol.path(), "foo.badext")

        os.mkfifo(subvol.path("a_fifo"))
        with self.assertRaisesRegex(AssertionError, " a file nor a dir"):
            deserialize_untyped(subvol.path(), "a_fifo")
