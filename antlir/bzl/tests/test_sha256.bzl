# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:sha256.bzl", "hex_to_base64", "sha256_b64")

def test_sha256():
    # The computations were checked via Python3, plus `.decode.strip('=')`:
    #   base64.urlsafe_b64encode(hashlib.sha256(b'foobar').digest())
    unittest.assert_eq(sha256_b64("foobar"), "w6uP8Tcg6K2QR905Rms8iXTlksL6OD1KOWBxTK7wxPI")

def test_hex_to_b64():
    # The computations were checked via Python3, plus `.decode.strip('=')`:
    #   base64.urlsafe_b64encode(b'\xfb')
    #   base64.urlsafe_b64encode(b'\xDE\xAD')
    #   base64.urlsafe_b64encode(b'\xde\xad\xbe')
    unittest.assert_eq(hex_to_base64("fb"), "-w")  # 2-byte final hex chunk
    unittest.assert_eq(hex_to_base64("DEAD"), "3q0")  # 1-byte final hex chunk; uppercase
    unittest.assert_eq(hex_to_base64("deadbe"), "3q2-")  # 3-byte final hex chunk
