#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
yum.conf files may specify `gpgkey` URLs for each repo. Here, we snapshot
the URLs used by a specific repo -- and verify that they already occur in a
previously version-controlled allowlist directory of GPG keys.  The intent
behind this secondary verification is to avoid blindly trusting the servers
(and transport layer) we use during the snapshot.
"""
import os
from typing import Iterable
from urllib.parse import urlparse

from antlir.fs_utils import create_ro, Path

from .open_url import open_url


def snapshot_gpg_keys(
    *, key_urls: Iterable[str], allowlist_dir: Path, snapshot_dir: Path
) -> None:
    os.mkdir(snapshot_dir / "gpg_keys")
    for url in key_urls:
        with open_url(url) as key_file:
            key_content = key_file.read()

        # Check that the key is in our allowlist, and the content matches.
        filename = os.path.basename(urlparse(url).path)
        with open(allowlist_dir / filename, "rb") as infile:
            allowlist_key = infile.read()
            assert allowlist_key == key_content, (allowlist_key, key_content)

        with create_ro(snapshot_dir / "gpg_keys" / filename, "wb") as outfile:
            outfile.write(allowlist_key)
