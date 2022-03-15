# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from http import HTTPStatus
from http.client import HTTPConnection, HTTPSConnection
from http.server import BaseHTTPRequestHandler
from shutil import copyfileobj
from ssl import SSLContext, create_default_context, CERT_REQUIRED
from typing import Optional, Callable
from urllib.parse import urlparse

from antlir.common import get_logger

_CHUNK_SIZE = 1024 * 1024
_ALWAYS_COPY_HEADERS = ["content-type", "content-length"]


log = get_logger()


def proxy_url(
    url: str,
    handler: BaseHTTPRequestHandler,
    *,
    headers_filter: Optional[Callable[[str], bool]] = None,
    allow_insecure_http: bool = False,
    context: Optional[SSLContext] = None,
) -> None:
    """
    Tries to retrieve requested url and proxy it to the provided handler.

    Use headers_filter function to check which headers should be copied.

    proxying http protocol will only work if allow_insecure_http is set to True.

    context is an optional ssl.SSLContext object. If not provided then default
    one is used. Default context requires valid remote certificate.
    """

    parsed_url = urlparse(url)
    if parsed_url.scheme == "https":
        if not context:  # pragma: no cover
            context = create_default_context()
            context.verify_mode = CERT_REQUIRED
            context.check_hostname = True
        conn = HTTPSConnection(
            parsed_url.netloc,
            context=context,
        )
    elif allow_insecure_http:
        conn = HTTPConnection(parsed_url.netloc)
    else:  # pragma: no cover
        raise RuntimeError(f"Attempt to proxy to unencrypted URL {url}")

    path = url[len(f"{parsed_url.scheme}://{parsed_url.netloc}") :]

    try:
        log.debug(
            f"Fetching /{path} from {parsed_url.scheme}://{parsed_url.netloc}"
        )
        conn.request("GET", f"{path}")
        rsp = conn.getresponse()
        log.debug(f"Response from remote: {rsp.status}")
        handler.send_response(rsp.status)

        content_length = 0
        for k, v in rsp.getheaders():
            k = k.lower()
            if k == "content-length":
                content_length = int(v)

            if k in _ALWAYS_COPY_HEADERS or (
                headers_filter and headers_filter(k)
            ):
                handler.send_header(k, v)
        handler.end_headers()

        chunk_size = (
            content_length
            if content_length and content_length < _CHUNK_SIZE
            else _CHUNK_SIZE
        )
        copyfileobj(rsp, handler.wfile, chunk_size)
    except Exception as e:  # pragma: no cover
        log.error(e)
        handler.send_error(HTTPStatus.INTERNAL_SERVER_ERROR, str(e))
    finally:
        conn.close()
