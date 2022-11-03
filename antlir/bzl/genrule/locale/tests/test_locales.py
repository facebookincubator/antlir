#!/usr/bin/python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import unittest


class LayerWithLocalesTestCase:
    def test_expected_locales(self) -> None:
        installed_locales = subprocess.check_output(
            [
                "/usr/bin/localedef",
                "--list-archive",
                # pyre-fixme[16]: `LayerWithLocalesTestCase` has no attribute
                #  `_TEST_LOCALE_ARCHIVE`.
                self._TEST_LOCALE_ARCHIVE,
            ],
            text=True,
        ).splitlines()

        # pyre-fixme[16]: `LayerWithLocalesTestCase` has no attribute `assertEqual`.
        # pyre-fixme[16]: `LayerWithLocalesTestCase` has no attribute `_TEST_LOCALES`.
        self.assertEqual(installed_locales, self._TEST_LOCALES.split(","))


class SingleLocaleArchiveTestCase(LayerWithLocalesTestCase, unittest.TestCase):
    _TEST_LOCALE_ARCHIVE = "/single-locale-archive"
    _TEST_LOCALES = "en_US.utf8"


class MultipleLocaleArchiveTestCase(LayerWithLocalesTestCase, unittest.TestCase):
    _TEST_LOCALE_ARCHIVE = "/multiple-locale-archive"
    _TEST_LOCALES = "en_CA.utf8,en_US.utf8"
