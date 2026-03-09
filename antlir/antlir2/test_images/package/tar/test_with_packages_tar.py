# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# pyre-strict

import importlib.resources
import tarfile
import unittest


class TestWithPackagesTar(unittest.TestCase):
    def test_has_rpm_db(self) -> None:
        with importlib.resources.open_binary(__package__, "with-packages.tar") as f:
            with tarfile.open(fileobj=f) as tar:
                self.assertIn("./usr/lib/sysimage/rpm", tar.getnames())
