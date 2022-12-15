# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

_format_enum = enum("sendstream", "sendstream.v2")

def receive_sendstream(
        *,
        src: str.type,
        format: str.type) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "receive_sendstream",
        sources = {
            "src": src,
        },
        kwargs = {
            "format": _format_enum(format),
        },
    )

def receive_sendstream_to_json(
        format: str.type,
        sources: {str.type: "artifact"}) -> {str.type: ""}:
    return {
        "format": format,
        "src": sources["src"],
    }
