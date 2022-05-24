# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import ssl
from http import HTTPStatus
from http.client import HTTPConnection, HTTPSConnection
from http.server import BaseHTTPRequestHandler
from shutil import copyfileobj
from typing import Callable, Optional
from urllib.parse import urlparse

from antlir.common import get_logger

_ALWAYS_COPY_HEADERS = ["content-type", "content-length"]


# pyre-fixme[5]: Global expression must be annotated.
log = get_logger()


def proxy_url(
    url: str,
    handler: BaseHTTPRequestHandler,
    *,
    headers_filter: Callable[[str], bool] = lambda h: False,
    allow_insecure_http: bool = False,
    context: Optional[ssl.SSLContext] = None,
) -> None:
    """
    Tries to retrieve requested url and proxy it to the provided handler.

    Use the `headers_filter` function to check which headers should be copied.

    Proxying http protocol will only work if `allow_insecure_http == True`.

    `context` is an optional ssl.SSLContext object. If not provided then a
    secure default is used, requiring a valid remote certificate.
    """

    parsed_url = urlparse(url)
    if parsed_url.scheme == "https":
        if not context:  # pragma: no cover
            # Default to a secure context.
            context = ssl.create_default_context(ssl.Purpose.SERVER_AUTH)
        conn = HTTPSConnection(parsed_url.netloc, context=context)
    elif allow_insecure_http:
        conn = HTTPConnection(parsed_url.netloc)
    else:  # pragma: no cover
        raise RuntimeError(f"Attempt to proxy to unencrypted URL {url}")

    url_prefix = f"{parsed_url.scheme}://{parsed_url.netloc}"
    assert url.startswith(url_prefix)

    try:
        log.debug(f"Fetching {url}")
        conn.request("GET", url[len(url_prefix) :])
        rsp = conn.getresponse()
        log.debug(f"Response from remote: {rsp.status}")
        handler.send_response(rsp.status)

        for k, v in rsp.getheaders():
            k = k.lower()
            if k in _ALWAYS_COPY_HEADERS or headers_filter(k):
                handler.send_header(k, v)
        handler.end_headers()

        copyfileobj(rsp, handler.wfile)
    except Exception as e:  # pragma: no cover
        log.error(e)
        handler.send_error(HTTPStatus.INTERNAL_SERVER_ERROR, str(e))
    finally:
        conn.close()
