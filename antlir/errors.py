# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


class AntlirError(Exception):
    backtrace_is_interesting: bool = False

    def __init__(self, msg, backtrace_is_interesting=None):
        super().__init__(msg)
        if backtrace_is_interesting is not None:
            self.backtrace_is_interesting = backtrace_is_interesting

    def __str__(self):
        # This prefix allows automation to pick high-signal errors out of the
        # logs (see D35687997).
        clsname = type(self).__name__
        if not clsname.endswith("Error"):
            clsname += "Error"
        return f"Antlir{clsname}: " + super().__str__()


class UserError(AntlirError):
    """A user-understandable Antlir error.
    This is probably something that the user caused, but should definitely be
    something that the end user is able to fix.
    For example, an error about a missing user should be a UserError (since the
    user should add that user to the current or some parent layer), but a
    transient RPM failure should not be a UserError (as there is nothing the
    user can do about that)
    """

    backtrace_is_interesting: bool = False


class InfraError(AntlirError):
    """An internal infra-related Antlir failure. The user probably cannot do
    anything about this, but Antlir has hit some unrecoverable error.

    Most errors of this class have nothing to do with Antlir python code, but
    instead occur in some external tool. When `backtrace_is_interesting` is
    False, the backtrace will be omitted from the error display unless
    ANTLIR_DEBUG is turned on.
    """

    backtrace_is_interesting: bool = False


class ToolMissingError(InfraError):
    """Raised when an expected CLI tool is missing from the host system"""

    backtrace_is_interesting: bool = False

    def __init__(self, tool) -> None:
        self.tool = tool
        super().__init__(f"Missing tool '{tool}'")
