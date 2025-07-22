# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/distro/deps:defs.bzl", "select_triple")

def libstdcxx_headers(version):
    """
    Helper macro to return the typical libstdc++ header paths, selectified based
    on arch.
    """
    return [
        "/usr/include/c++/{version}".format(version = version),
        "/usr/include/c++/{version}/backward".format(version = version),
    ] + select_triple(["/usr/include/c++/{version}/{{triple}}".format(version = version)])
