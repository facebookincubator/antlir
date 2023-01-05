# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/buck2/bzl:layer.bzl", "layer")
load("//antlir/buck2/bzl/feature:receive_sendstream.bzl", "receive_sendstream")

def layer_from_package(
        *,
        name: str.type,
        src: str.type,
        format: str.type,
        **kwargs):
    if "features" in kwargs:
        fail("'features' not allowed here")
    layer(
        name = name,
        features = [
            receive_sendstream(
                src = src,
                format = format,
            ),
        ],
        **kwargs
    )
