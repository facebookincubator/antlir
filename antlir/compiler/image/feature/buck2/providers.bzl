# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

FeatureInfo = provider(
    fields = [
        "inline_features",
    ],
)

ItemInfo = provider(
    fields = [
        "items",
    ],
)

def feature_provider(feature_key, feature_shape):
    return [
        DefaultInfo(),
        ItemInfo(items = struct(**{feature_key: [feature_shape]})),
    ]
