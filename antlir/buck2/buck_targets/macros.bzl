# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Shared helpers for the `buck_targets` round-trip machinery, used by BOTH the
`load_targets.bxl` dumper and by macro definitions that generate targets.

A `BuckTarget` may be a macro that expands into a primary target (which keeps the
macro's name and round-trips) plus one or more auxiliary targets. Those auxiliary
targets should carry a "macro-expanded" label so the dumper skips them: only
top-level targets round-trip.

The label format is `antlir-buck:macro-expanded:<original_rule>`. The dumper
matches on the prefix only; `<original_rule>` records which rule the macro
expanded from and is reserved for future use.
"""

_MACRO_EXPANDED_PREFIX = "antlir-buck:macro-expanded:"

def macro_expanded_label(original_rule):
    """The `labels` entry to attach to a macro's auxiliary (non-top-level)
    targets so `load_targets.bxl` skips them.

    Args:
        original_rule: the rule the macro expanded from (informational; reserved
            for future use).
    """
    return _MACRO_EXPANDED_PREFIX + original_rule

def is_macro_expanded_label(label):
    """Whether `label` marks a target as macro-expanded output."""
    return label.startswith(_MACRO_EXPANDED_PREFIX)
