# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# A repo whose snapshots record URLs it cannot download as-is maps them to
# ones it can, by URL scheme, in its root buckconfig:
#
#   [antlir2]
#   snapshot_url_formats = <scheme>=<format> ...
#
# For `<scheme>://[<user>@]<host>/<path>`, `<format>` gets `{user}`, `{host}`
# and `{path}`.
def _parse_formats(value: str) -> dict[str, str]:
    formats = {}
    for entry in value.split():
        scheme, sep, fmt = entry.partition("=")
        if not sep or not scheme or not fmt:
            fail("antlir2.snapshot_url_formats: expected <scheme>=<format>, got '{}'".format(entry))
        formats[scheme] = fmt
    return formats

_FORMATS = _parse_formats(read_root_config("antlir2", "snapshot_url_formats", ""))

def resolve_snapshot_url(url: str) -> str:
    scheme, sep, rest = url.partition("://")
    fmt = _FORMATS.get(scheme) if sep else None
    if not fmt:
        return url
    authority, _, path = rest.partition("/")
    user, _, host = authority.rpartition("@")
    return fmt.format(user = user, host = host, path = path)
