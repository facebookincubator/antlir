# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


class UserError(Exception):
    """A user-understandable Antlir error.
    This is probably something that the user caused, but should definitely be
    something that the end user is able to fix.
    For example, an error about a missing user should be a UserError (since the
    user should add that user to the current or some parent layer), but a
    transient RPM failure should not be a UserError (as there is nothing the
    user can do about that)
    """

    pass
