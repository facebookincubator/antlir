# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":target_helpers.bzl", "normalize_target")

_TargetTaggerInfo = provider(fields = ["targets"])

def _new_target_tagger():
    return _TargetTaggerInfo(targets = {})

def _tag_target(target_tagger, target, is_layer = False):
    target = normalize_target(target)
    target_tagger.targets[target] = 1  # Use a dict, since a target may recur
    return {("__BUCK_LAYER_TARGET" if is_layer else "__BUCK_TARGET"): target}

def _extract_tagged_target(tagged):
    return tagged.get("__BUCK_TARGET") or tagged["__BUCK_LAYER_TARGET"]

def _tag_required_target_key(tagger, d, target_key, is_layer = False):
    if target_key not in d:  # pragma: no cover
        fail(
            "{} must contain the key {}".format(d, target_key),
        )
    d[target_key] = _tag_target(tagger, target = d[target_key], is_layer = is_layer)

def _target_tagger_to_feature(target_tagger, items, extra_deps = None):
    return struct(
        items = items,
        # We need to tell Buck that we depend on these targets, so
        # that `image_layer` can use `deps()` to discover its
        # transitive dependencies.
        #
        # This is a little hacky, because we are forcing these
        # targets to be built or fetched from cache even though we
        # don't actually use them until a later build step --- which
        # might be on a different host.
        #
        # Future: Talk with the Buck team to see if we can eliminate
        # this inefficiency.
        deps = list(target_tagger.targets.keys()) + (extra_deps or []),
    )

target_tagger_helper = struct(
    new_target_tagger = _new_target_tagger,
    tag_target = _tag_target,
    tag_required_target_key = _tag_required_target_key,
    extract_tagged_target = _extract_tagged_target,
    target_tagger_to_feature = _target_tagger_to_feature,
)
