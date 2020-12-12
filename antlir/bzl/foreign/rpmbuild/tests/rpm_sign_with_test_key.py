#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import subprocess
import sys
import tempfile


def sign_with_test_key(rpm: str) -> None:
    # Since we're using a test key, create a temporary directory to house the
    # gpg configuration and trust data so as not to pollute the user's host
    # data.
    with tempfile.TemporaryDirectory() as gnupg_home, importlib.resources.path(
        __package__, "gpg-test-signing-key"
    ) as signing_key:
        subprocess.run(
            ["gpg", "-q", "--import", signing_key],
            env={"GNUPGHOME": gnupg_home},
            check=True,
        )
        subprocess.run(
            [
                "rpmsign",
                "--addsign",
                "--define",
                "_gpg_name Test Key",
                "--define",
                "_gpg_digest_algo sha256",
                rpm,
            ],
            check=True,
        )


if __name__ == "__main__":
    sign_with_test_key(sys.argv[1])
