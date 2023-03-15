# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

InlineFeatureInfo = provider(fields = [
    # Inline features must provide some way to uniquely identify themselves so
    # that coerced attributes like deps can be passed back to the feature itself.
    # The key does not need to be stable for a given set of inputs to the
    # feature (because it is not serialized anywhere), it just must be unique
    # for a single evaluation. `hash_key` is provided for convenience and should
    # suffice for all use cases.
    "key",
    "feature_type",
    "sources",
    "deps",
    "kwargs",
    "to_json",
])
