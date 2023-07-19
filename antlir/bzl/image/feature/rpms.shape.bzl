# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_tagger.shape.bzl", "target_tagged_image_source_t")

action_t = shape.enum("install", "remove_if_exists")

version_set_t = shape.union(
    # This string corresponds to `version_set_allow_all_versions`.
    str,
    shape.dict(str, str),
)

rpm_action_item_t = shape.shape(
    action = action_t,
    flavor_to_version_set = shape.dict(str, version_set_t),
    source = shape.field(target_tagged_image_source_t, optional = True),
    name = shape.field(str, optional = True),
    antlir1_i_know_what_im_doing_arch = shape.field(str, optional = True),
    subjects_src = shape.field(target_tagged_image_source_t, optional = True),
)
