# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import tarfile
import unittest


class Test(unittest.TestCase):
    def test_tar(self) -> None:
        infos = {}
        with importlib.resources.open_binary(
            __package__, "test.tar"
        ) as f, tarfile.open(fileobj=f) as tar:
            for member in tar:
                infos[member.name] = member
            baz = tar.extractfile("./foo/bar/baz")
            self.assertIsNotNone(baz)
            self.assertEqual(baz.read(), b"baz\n")

        self.assertEqual(infos["./baz-sym"].linkname, "/foo/bar/baz")
        self.assertEqual(infos["./foo/bar"].type, tarfile.DIRTYPE)
        self.assertEqual(infos["./foo/bar/baz"].type, tarfile.REGTYPE)
        self.assertEqual(infos["./foo/bar/baz"].uid, 42)
