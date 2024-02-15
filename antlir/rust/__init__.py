# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


# Re-export everything from the Rust extension module. Annoyingly, this is
# necessary to make any `python_{binary,library,unittest}` targets work under
# antlir/rust, otherwise Buck will generate an __init__.py in this directory
# that would make the Rust extension effectively invisible to Python.

from antlir.rust.native_antlir_impl import *  # noqa
