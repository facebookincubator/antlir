# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

_B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
_E16 = list(enumerate(["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f"]))

def _hex_triple_to_b64_pair(i1, i2, i3):
    x = 256 * i1 + 16 * i2 + i3
    return _B64[x // 64] + _B64[x % 64]

_HEX123_TO_B64 = {
    h1 + h2 + h3: _hex_triple_to_b64_pair(i1, i2, i3)
    for i1, h1 in _E16
    for i2, h2 in _E16
    for i3, h3 in _E16
}
_HEX123_TO_B64.update({
    h1 + h2: _hex_triple_to_b64_pair(i1, i2, 0)
    for i1, h1 in _E16
    for i2, h2 in _E16
})
_HEX123_TO_B64.update({
    # The second b64 digit will always be `A` aka 0.
    h1: _hex_triple_to_b64_pair(i1, 0, 0)[0]
    for i1, h1 in _E16
})

# This differs from real base64 in that it omits the vestigial `=` padding,
# thus minimizing filename lengths. See `self_test` for examples.
def hex_to_base64(x):
    chunks = []
    for i in range(0, len(x), 3):
        chunks.append(_HEX123_TO_B64[x[i:i + 3].lower()])
    return "".join(chunks)

# The return value has 6 bits per byte, except the last byte has 4 bits.
def sha256_b64(s):
    # @lint-ignore BUCKLINT
    return hex_to_base64(native.sha256(s))

def sha256_hex(s):
    # @lint-ignore BUCKLINT
    return native.sha256(s)
