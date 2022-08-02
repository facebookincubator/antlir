# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Our continuous integration system might run different build steps in
different sandboxes, so the intermediate outputs of `feature`s
must be cacheable by Buck.  In particular, they must not contain
absolute paths to targets.

However, to build a dependent `image_layer`, we will need to invoke the
image compiler with the absolute paths of the outputs that will comprise
the image.

Therefore, we need to (a) record all the targets, for which the image
compiler will need absolute paths, and (b) resolve them only in the
build step that invokes the compiler.

This tagging scheme makes it possible to find ALL such targets in the
output of `feature` targets by simply traversing the JSON structure.  This
seems more flexible and less messy than maintaining a look-aside list of
targets whose paths the `image_layer` converter would need to resolve.
"""

load(":image_source.bzl", "image_source")
load(":shape.bzl", "shape")
load(":target_tagger.shape.bzl", "target_tagged_image_source_t")
load(":target_tagger_helper.bzl", "target_tagger_helper")
load(":wrap_runtime_deps.bzl", "maybe_wrap_executable_target")

new_target_tagger = target_tagger_helper.new_target_tagger
tag_target = target_tagger_helper.tag_target
tag_required_target_key = target_tagger_helper.tag_required_target_key
extract_tagged_target = target_tagger_helper.extract_tagged_target
target_tagger_to_feature = target_tagger_helper.target_tagger_to_feature

def tag_and_maybe_wrap_executable_target(target_tagger, target, wrap_suffix, **kwargs):
    was_wrapped, wrapped_target = maybe_wrap_executable_target(
        target,
        wrap_suffix,
        **kwargs
    )
    return was_wrapped, tag_target(target_tagger, wrapped_target)

def image_source_as_target_tagged_dict(target_tagger, user_source):
    src = shape.DEPRECATED_as_dict_for_target_tagger(image_source(user_source))
    is_layer = src["layer"] != None
    tag_required_target_key(
        target_tagger,
        src,
        "layer" if is_layer else "source",
        is_layer = is_layer,
    )
    return src

def image_source_as_target_tagged_t(target_tagger, user_source):
    return target_tagged_image_source_t(
        **image_source_as_target_tagged_dict(target_tagger, user_source)
    )
