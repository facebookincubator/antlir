# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
A trivial rule used only by the `buck_targets` round-trip integration test. Its
attrs (`srcs`, `deps`) mirror the strict `MyRule` struct in
`tests/roundtrip.rs`. `deps` is a plain list-of-string (not real deps) so the
fixture needs no other targets to exist.
"""

def _my_rule_impl(_ctx):
    return [DefaultInfo()]

my_rule = rule(
    impl = _my_rule_impl,
    attrs = {
        "deps": attrs.list(attrs.string(), default = []),
        "labels": attrs.list(attrs.string(), default = []),
        "srcs": attrs.list(attrs.string(), default = []),
    },
)
