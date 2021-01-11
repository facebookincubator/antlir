# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Forwards-and-backwards compatible utilities for dealing with structs.
"""

def _is_struct(s):
    return hasattr(s, "_asdict") or hasattr(s, "to_json")

def struct_to_dict(s):
    if hasattr(s, "_asdict"):
        return dict(s._asdict())

    # both java starlark and rust starlark add a couple of extra things to the
    # results of dir(some_struct) strip those out.
    return {attr: getattr(s, attr) for attr in dir(s) if attr not in ["to_json", "to_proto"]}

structs = struct(
    to_dict = struct_to_dict,
    is_struct = _is_struct,
)
