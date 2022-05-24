# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from ..errors import UserError
from ..signed_source import (
    assert_signed_source,
    sign_source,
    signed_source_sigil,
)
from .common import AntlirTestCase


_HELLO_WORLD = "hello SignedSource<<9f94b0b1eddcee39813128cd51ef0e47>> world!"


class SignedSourceTestCase(AntlirTestCase):
    # pyre-fixme[3]: Return type must be annotated.
    def test_sign(self):
        self.assertEqual(
            _HELLO_WORLD,
            sign_source(f"hello {signed_source_sigil()} world!"),
        )

    # pyre-fixme[3]: Return type must be annotated.
    def test_sign_error(self):
        with self.assertRaisesRegex(
            RuntimeError,
            r"^First .* lack `signed_source_sigil\(\)`: aaa$",
        ):
            sign_source("aaa")

    # pyre-fixme[3]: Return type must be annotated.
    def test_verify(self):
        assert_signed_source(_HELLO_WORLD, "_HELLO_WORLD")
        with self.assertRaisesRegex(
            UserError, "Invalid signed source: BAD1. .* lacks a SignedSource "
        ):
            assert_signed_source("look ma, no token", "BAD1")
        with self.assertRaisesRegex(
            UserError, "Invalid signed source: BAD2. .* should explain "
        ):
            assert_signed_source(_HELLO_WORLD + " bye.", "BAD2")
