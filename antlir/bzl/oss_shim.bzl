# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Starlark checks lots of code before running (unlike Python), so hiding broken
# code beneath an `if` that's guaranteed to evaluate `False` in OSS is not
# enough to satisfy buck2.
#
# This bzl file is provided to make split internal/oss loads a little easier
#

def ret_none(*args, **kwargs):
    return None

def ret_empty_list(*args, **kwargs):
    return []

empty_dict = {}
empty_list = []

none = None

special_tags = struct(
    run_as_bundle = "OSS_NO_OP",
    enable_artifact_reporting = "OSS_NO_OP",
)

fully_qualified_test_name_rollout = struct(
    use_fully_qualified_name = lambda: False,
)

NAMING_ROLLOUT_LABEL = "OSS_NO_OP"
