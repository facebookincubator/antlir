# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Feature rules in buck2
======================

Image features in buck2 are coalesced into a single rule for each image that
provides a `FeatureInfo`. This single concrete rule can be constructed with a
combination of inline or standalone features.
Inline features are simply instances of the `InlineFeatureInfo` provider, while
standalone features are concrete targets that provide `FeatureInfo` themselves.

Usage
=====
The only way for an image build user to construct features is using the inline
feature macros (defined in `.bzl` files in this directory). These inline
features are given to a `feature` rule (or directly to `layer`).

The `layer` rule always creates a single `feature` rule internally, combining
all the inline and standalone features into a single input for the compiler.

Feature implementations
=======================
Features are implemented via macros that take user input and transform it to be
usable with a `feature` rule.
Since rule attribute coercion only happens at the time a real rule is called,
not on inline feature construction, the internal structure of inline rules is a
bit complicated.

Inline feature macros must return an `InlineFeatureInfo` provider, that is then
used to reconstruct compiler-JSON on the other end.
The `InlineFeatureInfo` contains:
    - feature_type: type disambiguation for internal macros and compiler
    - sources: map of key -> source for `attrs.source()` dependencies needed by
        the feature.
    - deps: map of key -> dep for `attrs.dep()` dependencies needed by the
        feature.
    - kwargs: map of all non-dependency inputs
For `sources` and `deps`, the user input to the inline feature input will just
be a simple string that is a label (or path for `sources`), but by including it
in the special maps in `InlineFeatureInfo`, the `feature` rule is able to coerce
those labels to concrete artifacts.

Image features must also provide a function to convert the kwargs, sources and
deps into a JSON struct readable by the compiler. This function must then be
added to the `_feature_to_json` map in this file.
"""

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:flatten.bzl", "flatten")
load(":feature_info.bzl", "InlineFeatureInfo")
load(":install.bzl", "install_to_json")
load(":mount.bzl", "mount_to_json")
load(":rpms.bzl", "rpms_to_json")
load(":usergroup.bzl", "group_to_json", "user_to_json", "usermod_to_json")

Features = transitive_set()
FeatureDeps = transitive_set()

FeatureInfo = provider(fields = [
    # FeatureDeps transitive set
    # All the targets that must be materialized on disk for the compiler to be
    # able to build this feature
    "deps",
    # Features transitive set
    # List of output files that contain lists of features deserializable by
    # Antlir tools. Files include inline features in this rule, as well as all
    # the features this one brings in via deps
    "json_files",
])

_feature_to_json = {
    "group": group_to_json,
    "install": install_to_json,
    "mount": mount_to_json,
    "rpm": rpms_to_json,
    "user": user_to_json,
    "usermod": usermod_to_json,
}

def _impl(ctx: "context") -> ["provider"]:
    # Merge inline features into a single JSON file
    inline_features = []
    inline_deps = []
    for key, inline in ctx.attrs.inline_features.items():
        inline = InlineFeatureInfo(**json.decode(inline))
        feature_sources = ctx.attrs.inline_features_sources[key]
        feature_deps = ctx.attrs.inline_features_deps[key]
        inline_deps.extend(feature_sources.values())
        for dep in feature_deps.values():
            inline_deps.extend(dep[DefaultInfo].default_outputs)

        feature_json = _feature_to_json[inline.feature_type](sources = feature_sources, deps = feature_deps, **inline.kwargs)
        feature_json["__feature_type"] = inline.feature_type
        inline_features.append(feature_json)
    json_out = ctx.actions.write_json("features.json", inline_features)

    # Track the JSON outputs and deps of other feature targets with transitive
    # sets. Note that we cannot produce a single JSON file with all the
    # transitive features, because we need to support "genrule" features where a
    # command outside of buck can be used to produce much more dynamic feature
    # JSON (for example, extract.bzl requires Rust logic to produce its feature
    # output)
    json_files = ctx.actions.tset(
        Features,
        value = [json_out],
        children = [f[FeatureInfo].json_files for f in ctx.attrs.feature_targets],
    )
    deps = ctx.actions.tset(
        FeatureDeps,
        value = inline_deps,
        children = [f[FeatureInfo].deps for f in ctx.attrs.feature_targets],
    )

    return [
        FeatureInfo(
            json_files = json_files,
            deps = deps,
        ),
        DefaultInfo(default_outputs = [json_out]),
    ]

_feature = rule(
    impl = _impl,
    attrs = {
        # feature targets are instances of `_feature` rules that are merged into
        # the output of this rule
        "feature_targets": attrs.list(
            attrs.dep(providers = [FeatureInfo]),
        ),
        # inline features are direct instances of the FeatureInfo provider
        "inline_features": attrs.dict(
            # Unique key for this feature (see _hash_key below)
            attrs.string(),
            # This is really a json-serialized FeatureInfo, validated by the
            # type hint on the `feature` macro below
            attrs.string(),
        ),
        # Features need a way to coerce strings to sources or dependencies.
        # Map "feature key" -> "feature deps"
        "inline_features_deps": attrs.dict(attrs.string(), attrs.dict(attrs.string(), attrs.dep())),
        # Map "feature key" -> "feature sources"
        "inline_features_sources": attrs.dict(attrs.string(), attrs.dict(attrs.string(), attrs.source())),
    },
)

def feature(
        name: str.type,
        # accept FeatureInfo providers, string labels of other feature targets or lists of the same
        # starlark type annotations do not allow for arbitrary levels of
        # recursion, so just handwrite nesting down to a few levels
        features: [["InlineFeatureInfo", str.type, ["InlineFeatureInfo"]]],
        visibility = None):
    """
    Create a target representing a collection of one or more image features.

    `features` is a list that can contain either:
        - inline (aka unnamed) features created with macros like `install()`
        - labels referring to other `feature` targets
    """
    features = flatten.flatten(features, item_type = ["InlineFeatureInfo", str.type])
    inline_features = {}
    feature_targets = []
    inline_features_deps = {}
    inline_features_sources = {}
    for feat in features:
        if types.is_string(feat):
            feature_targets.append(feat)
        else:
            feature_key = _hash_key(feat.kwargs)
            inline_features_deps[feature_key] = feat.deps or {}
            inline_features_sources[feature_key] = feat.sources or {}
            inline_features[feature_key] = json.encode(feat)

    return _feature(
        name = name,
        feature_targets = feature_targets,
        inline_features = inline_features,
        inline_features_deps = inline_features_deps,
        inline_features_sources = inline_features_sources,
        visibility = visibility,
    )

# We need a way to disambiguate inline features so that deps/sources can be
# passed back to them to convert to compiler json. This isn't persisted anywhere
# and does not end up in any target labels, so it does not need to be stable,
# just unique for a single evaluation of the target graph.
def _hash_key(kwargs) -> str.type:
    return sha256(json.encode(kwargs))
