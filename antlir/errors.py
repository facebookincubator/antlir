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

    # pyre-fixme[3]: Return type must be annotated.
    def __str__(self):
        # This prefix allows automation to pick high-signal errors out of the
        # logs (see D35687997).
        return "AntlirUserError: " + super().__str__()


class ToolMissing(Exception):
    """Raised when an expected CLI tool is missing from the host system"""

    # pyre-fixme[2]: Parameter must be annotated.
    def __init__(self, tool) -> None:
        # pyre-fixme[4]: Attribute must be annotated.
        self.tool = tool
        super().__init__(f"Missing tool '{tool}'")
