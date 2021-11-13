# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")

def fail_with_context(msg, context):
    if types.is_none(context):
        fail(msg)
    elif types.is_string(context):
        fail("{}{}".format(context + ":" if context else "", msg))
    elif types.is_list(context):
        stack = [msg] + context[::-1] if context else [msg]
        fail("\n When: ".join(stack))
    else:
        fail("Provided invalid context {} when trying to render error: {}".format(context, msg))

def add_context(msg, context = None):
    if msg == None:
        return context
    context = context or []
    return context + [msg]
