# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: is_facebook = True
is_facebook = False # @oss-enable

def internal_external(*, fb, oss):
    if is_facebook:
        return fb
    else:
        return oss
