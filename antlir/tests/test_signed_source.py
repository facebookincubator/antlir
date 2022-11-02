# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.signed_source import (
    sign_source,
    signed_source_sigil,
    SignedSourceError,
)
from antlir.tests.common import AntlirTestCase


_HELLO_WORLD = "hello SignedSource<<9f94b0b1eddcee39813128cd51ef0e47>> world!"


class SignedSourceTestCase(AntlirTestCase):
    def test_sign(self):
        self.assertEqual(
            _HELLO_WORLD,
            sign_source(f"hello {signed_source_sigil()} world!"),
        )

    def test_sign_error(self):
        with self.assertRaisesRegex(
            SignedSourceError,
            "missing SignedSource token",
        ):
            sign_source("aaa")
