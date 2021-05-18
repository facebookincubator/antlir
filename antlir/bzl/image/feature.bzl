# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"This provides a more friendly UI to the feature.* macros."

load("//antlir/bzl/image/feature:new.bzl", "feature_new")
load("//antlir/bzl/image/feature:usergroup.bzl", "image_feature_group_add", "image_feature_user_add")

feature = struct(
    group_add = image_feature_group_add,
    new = feature_new,
    user_add = image_feature_user_add,
)
