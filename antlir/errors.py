# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import traceback
from typing import List


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


class ToolMissing(Exception):
    """Raised when an expected CLI tool is missing from the host system"""

    def __init__(self, tool):
        self.tool = tool
        super().__init__(f"Missing tool '{tool}'")


class SerializedException(Exception):
    exception: Exception
    traceback: List[str]

    def __init__(self, exception: Exception, traceback: List[str]):
        self.exception = exception
        self.traceback = traceback
        super().__init__(self.exception, self.traceback)


def serialize_exception(e: Exception) -> SerializedException:
    return SerializedException(
        exception=e,
        traceback=[l.rstrip() for l in traceback.format_tb(e.__traceback__)],
    )
