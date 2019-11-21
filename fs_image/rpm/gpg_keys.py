#!/usr/bin/env python3
'''
yum.conf files may specify `gpgkey` URLs for each repo. Here, we snapshot
the URLs used by a specific repo -- and verify that they already occur in a
previously version-controlled whitelist directory of GPG keys.  The intent
behind this secondary verification is to avoid blindly trusting the servers
(and transport layer) we use during the snapshot.
'''
import os

from typing import Iterable
from urllib.parse import urlparse

from .common import Path, create_ro
from .open_url import open_url


def snapshot_gpg_keys(
    *, key_urls: Iterable[str], whitelist_dir: Path, snapshot_dir: Path,
):
    os.mkdir(snapshot_dir / 'gpg_keys')
    for url in key_urls:
        with open_url(url) as key_file:
            key_content = key_file.read()

        # Check that the key is in our whitelist, and the content matches.
        filename = os.path.basename(urlparse(url).path)
        with open(whitelist_dir / filename, 'rb') as infile:
            whitelist_key = infile.read()
            assert whitelist_key == key_content, (whitelist_key, key_content)

        with create_ro(snapshot_dir / 'gpg_keys' / filename, 'wb') as outfile:
            outfile.write(whitelist_key)
