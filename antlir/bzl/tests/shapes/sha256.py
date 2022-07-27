# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import base64
import hashlib


def sha256_b64(s: str) -> str:
    """
    So we don't have have to deal with importing `sha256.bzl` to test
    `shape.hash`.
    """
    return (
        base64.urlsafe_b64encode(hashlib.sha256(str.encode(s)).digest())
        .decode()
        .strip("=")
    )
