# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def _quote(s):
    """
    Quote the input string to make it safe for use in a shell line (eg a
    genrule's 'bash' attr)
    """
    return "'" + s.replace("'", "'\\''") + "'"

shell = struct(
    quote = _quote,
)
