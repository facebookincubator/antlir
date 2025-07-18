# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select")
load("//antlir/antlir2/bzl:selects.bzl", "selects")

def select_format_triple(to_format):
    """
    Format the value in `to_format` based on the arch-specific gcc triple
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

def libstdcxx_headers(version, prefix = None):
    """
    Helper macro to return the typical libstdc++ header paths, selectified based
    on arch.
    """
    return [
        "/usr/include/c++/{version}".format(version = version),
        "/usr/include/c++/{version}/backward".format(version = version),
    ] + select_format_triple(["/usr/include/c++/{version}/{{triple}}".format(version = version)])
