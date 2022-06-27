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

RpmInfo = provider(
    fields = [
        "action",
        "flavors",
    ],
)

def feature_provider(feature_key, feature_shape):
    return [
        DefaultInfo(),
        ItemInfo(items = struct(**{feature_key: [feature_shape]})),
    ]

def rpm_provider(rpm_action_items, action, flavors):
    return [
        DefaultInfo(),
        ItemInfo(items = struct(**{"rpms": rpm_action_items})),
        RpmInfo(
            action = action,
            flavors = flavors,
        ),
    ]
