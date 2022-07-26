# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# pyre-strict

import unittest

from antlir.fs_utils import Path
from antlir.fs_utils_rs import Path as RustPath


class TestFsUtils(unittest.TestCase):
    def test_path_roundtrip(self) -> None:
        for path in [
            "/hello/world",
            b"/hello/world",
            Path("/hello/world"),
            Path(b"/hello/world"),
        ]:
            py_path = Path(path)
            rust_path = RustPath(path)
            self.assertEqual(type(rust_path), Path)
            self.assertEqual(rust_path, py_path)

    def test_bad_type(self) -> None:
        with self.assertRaisesRegex(TypeError, "42 is neither bytes nor str"):
            RustPath(42)  # pyre-ignore[6]

    def test_not_utf8(self) -> None:
        seq = b"\xfc\xa1\xa1\xa1\xa1\xa1"
        with self.assertRaises(UnicodeDecodeError):
            seq.decode("utf-8")
        # this is an invalid utf-8 sequence, but rust is using a PathBuf so it's
        # all ok
        RustPath(seq)
