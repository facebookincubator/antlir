# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":build_defs.bzl", "is_buck2")

"""
Forwards-and-backwards compatible utilities for dealing with structs.
"""

def _is_struct(s):
    return type(s) == type(struct()) or hasattr(s, "_asdict") or hasattr(s, "to_json")

def struct_to_dict(s):
    if hasattr(s, "_asdict"):
        return dict(s._asdict())

    # both java starlark and rust starlark add a couple of extra things to the
    # results of dir(some_struct) strip those out.
    return {attr: getattr(s, attr) for attr in dir(s) if attr not in ["to_json", "to_proto"]}

def _as_json(s):
    if is_buck2():
        # To avoid a warning about not using native
        my_native = native
        return my_native.json.encode(s)
    else:
        return s.to_json()

structs = struct(
    to_dict = struct_to_dict,
    # Important: We can't call this to_json, since some structs already
    # have to_json as a member, so have to call it as_json
    as_json = _as_json,
    is_struct = _is_struct,
)
