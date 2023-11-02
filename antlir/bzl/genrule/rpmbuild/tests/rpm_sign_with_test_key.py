#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
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
    # TODO(ls): This usage of `/tmp` for the dir is to work around an issue with
    # gpg and buck2.  Buck2 overrides the TMPDIR variable to put tmp files in a
    # managed location on disk, this path though can be quite long which gpg
    # does not like and fails with:
    #  Stderr: gpg: can't connect to the agent: File name too long
    #
    with tempfile.TemporaryDirectory(
        dir="/tmp"
    ) as gnupg_home, importlib.resources.path(
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
            env={"GNUPGHOME": gnupg_home},
            check=True,
        )


def main() -> None:
    sign_with_test_key(sys.argv[1])


if __name__ == "__main__":
    main()
