# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:selects.bzl", "selects")

def select_triple(to_format):
    """
    Format the value in `to_format` based on the arch-specific compiler triple
    (ex: x86_64-redhat-linux).
    """

    def _format_helper(triple):
        if isinstance(to_format, list):
            return [s.format(triple = triple) for s in to_format]
        else:
            return to_format.format(triple = triple)

    return selects.apply(
        arch_select(aarch64 = "aarch64-redhat-linux", x86_64 = "x86_64-redhat-linux"),
        _format_helper,
    )

def format_select(to_format: typing.Any, **kwargs) -> Select:
    """
    Formats `to_format` according to selects in `kwargs`.
    """

    def _format_helper(fmt: typing.Any, *args, **kwargs):
        if isinstance(fmt, list):
            return [_format_helper(e, *args, **kwargs) for e in fmt]
        elif isinstance(fmt, tuple):
            return tuple([_format_helper(e, *args, **kwargs) for e in fmt])
        else:
            return fmt.format(*args, **kwargs)

    return selects.apply(
        selects.join(**kwargs),
        lambda sels: _format_helper(
            to_format,
            **{a: getattr(sels, a) for a in dir(sels)}
        ),
    )
