#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from contextlib import contextmanager
from io import BytesIO
from typing import Final, Iterator, List, Type

import requests
from libfb.py.decorators import retryable

RETRYABLE_EXCEPTIONS: Final[List[Type]] = [
    requests.exceptions.Timeout,
    requests.exceptions.ConnectionError,
]


@contextmanager
@retryable(num_tries=3, sleep_time=1.0, retryable_exs=RETRYABLE_EXCEPTIONS)
def open_url(url: str) -> Iterator[BytesIO]:
    # pyre-fixme[16]: Module `utils` has no attribute `urlparse`.
    parsed_url = requests.utils.urlparse(url)
    if parsed_url.scheme == "file":
        assert parsed_url.netloc == "", f"Bad file URL: {url}"
        # pyre-fixme[16]: Module `utils` has no attribute `unquote`.
        with open(requests.utils.unquote(parsed_url.path), "rb") as infile:
            # pyre-fixme[7]: Expected `Iterator[BytesIO]` but got
            #  `Generator[io.BufferedReader, None, None]`.
            yield infile
    elif parsed_url.scheme in ["http", "https"]:
        # verify=True is the default, but I want to be explicit about HTTPS,
        # since this function receives GPG key material.
        with requests.get(url, stream=True, verify=True) as r:
            r.raise_for_status()
            yield r.raw  # A file-like `io`-style object for the HTTP stream
            if r.raw.isclosed():  # Proxy for "all data was consumed"
                # Sadly, requests 2.x does not verify content-length :/
                # We could check r.raw.length_remaining, likely equivalent.
                actual_size = r.raw.tell()
                header_size = int(r.headers["content-length"])
                assert actual_size == header_size, (actual_size, header_size)
    else:  # pragma: no cover
        raise RuntimeError(f"Unknown URL scheme in {url}")
