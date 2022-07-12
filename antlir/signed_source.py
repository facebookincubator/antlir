# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import hashlib
import re

from antlir.errors import UserError

_N_HEADER = 2048  # NB: The current linter does whole-file replacements


def signed_source_sigil() -> str:
    return "<<SignedSource::*O*zOeWoEQle#+L!plEphiEmie@IsG>>"  # Value lint uses


def sign_source(src: str) -> str:
    """
    `signed_source_sigil()` must occur in the header of `src`.

    We'll replace the sigil with the MD5 of the contents of the source.
    Lint will error if the MD5 in the header does not match the contents.

    This is not a security measure.  It is only intended to discourage
    people from manually fixing generated files, which is error-prone.
    """
    sigil = signed_source_sigil()
    try:
        idx = src.index(sigil, 0, _N_HEADER)
    except ValueError:
        raise RuntimeError(
            f"First {_N_HEADER} bytes of `src` lack `signed_source_sigil()`: "
            + src[:_N_HEADER]
        )

    md5hex = hashlib.md5(src.encode()).hexdigest()
    return src[:idx] + f"SignedSource<<{md5hex}>>" + src[idx + len(sigil) :]


def assert_signed_source(signed_src: str, which: str) -> None:
    """
    Raises `UserError` if `signed_src` does not have the right checksum.
    `which` must be a human-readable description of how to find the file
    being checked.
    """
    m = re.search("SignedSource<<[a-f0-9]{32}>>", signed_src[:_N_HEADER])
    if not m:
        raise UserError(
            f"Invalid signed source: {which}. The file's header lacks a "
            "SignedSource token, please revert it to trunk, and re-generate "
            "as described in the header."
        )

    if signed_src != sign_source(
        signed_src[: m.start()] + signed_source_sigil() + signed_src[m.end() :]
    ):
        raise UserError(
            f"Invalid signed source: {which}. The file's header should "
            "explain how to re-generate it correctly."
        )
