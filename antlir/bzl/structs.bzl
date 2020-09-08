"""
A forwards-and-backwards compatible wrapper around
@bazel_syklib//lib:structs.bzl that supports both Starlark and Python
runtimes.
To be removed when fbcode switches to Starlark-only parsing.
"""

load("@bazel_skylib//lib:structs.bzl", skylib_structs = "structs")

def _to_dict(s):
    # the python runtime provides this convenient function
    if hasattr(s, "_asdict"):
        return dict(**s._asdict())
    return skylib_structs.to_dict(s)

def _is_struct(s):
    return hasattr(s, "_asdict") or hasattr(s, "to_json")

structs = struct(
    to_dict = _to_dict,
    is_struct = _is_struct,
)
