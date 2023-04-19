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
Inline features are simply instances of the `ParseTimeFeature` record, while
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

Inline feature macros must return an `ParseTimeFeature` record, that is then
used to reconstruct compiler-JSON on the other end.
The `ParseTimeFeature` contains:
    - feature_type: type disambiguation for internal macros and compiler
    - deps_or_sources: map of key -> source for
        `attrs.one_of(attrs.dep(), attrs.source())` dependencies needed by the
        feature. The feature is always able to get the "artifact", and will be
        able to get provider details on "dependency" deps
    - deps: map of key -> dep for `attrs.dep()` dependencies needed by the
        feature.
    - kwargs: map of all non-dependency inputs
For `deps_and_sources` and `deps`, the user input to the inline feature input
will just be a simple string that is a label (or path for plain source files),
but by including it in the special maps in `ParseTimeFeature`, the `feature`
rule is able to coerce those labels to concrete artifacts.

Image features must also provide a function to convert the kwargs, sources and
deps into a JSON struct readable by the compiler. This function must then be
added to the `_feature_to_json` map in this file.
"""

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo")
# @oss-disable
# @oss-disable
load("//antlir/bzl:flatten.bzl", "flatten")
load(":clone.bzl", "clone_to_json")
load(":ensure_dirs_exist.bzl", "ensure_dir_exists_to_json")
load(":extract.bzl", "extract_to_json")
load(":genrule.bzl", "genrule_to_json")
load(":install.bzl", "install_to_json")
load(":mount.bzl", "mount_to_json")
load(":remove.bzl", "remove_to_json")
load(":requires.bzl", "requires_to_json")
load(":rpms.bzl", "rpms_to_json")
load(":symlink.bzl", "symlink_to_json")
load(":tarball.bzl", "tarball_to_json")
load(":usergroup.bzl", "group_to_json", "user_to_json", "usermod_to_json")

def _project_as_feature_json(value: ["artifact"]):
    return cmd_args(value, format = "--feature-json={}")

Features = transitive_set(args_projections = {"feature_json": _project_as_feature_json})
FeatureDeps = transitive_set()

_feature_to_json = {
    "clone": clone_to_json,
    "ensure_dir_exists": ensure_dir_exists_to_json,
    "ensure_dir_symlink": symlink_to_json,
    "ensure_file_symlink": symlink_to_json,
    "extract": extract_to_json,
    # @oss-disable
    # @oss-disable
    "genrule": genrule_to_json,
    "group": group_to_json,
    "install": install_to_json,
    "mount": mount_to_json,
    "remove": remove_to_json,
    "requires": requires_to_json,
    "rpm": rpms_to_json,
    "tarball": tarball_to_json,
    "user": user_to_json,
    "user_mod": usermod_to_json,
}

def _impl(ctx: "context") -> ["provider"]:
    # Merge inline features into a single JSON file
    inline_features = []
    inline_deps = []
    for key, inline in ctx.attrs.inline_features.items():
        feature_deps = ctx.attrs.inline_features_deps.get(key, None)
        feature_deps_or_sources = ctx.attrs.inline_features_deps_or_sources.get(key, None)
        if feature_deps:
            inline_deps.extend(feature_deps.values())
        if feature_deps_or_sources:
            inline_deps.extend(feature_deps_or_sources.values())

        to_json_kwargs = inline["kwargs"]
        if feature_deps != None:
            to_json_kwargs["deps"] = feature_deps
        if feature_deps_or_sources != None:
            to_json_kwargs["deps_or_sources"] = feature_deps_or_sources

        feature_json = _feature_to_json[inline["feature_type"]](**to_json_kwargs)
        if type(feature_json) == "record":
            feature_json = {
                k: getattr(feature_json, k)
                for k in dir(feature_json)
            }
        if "__feature_type" not in feature_json:
            feature_json["__feature_type"] = inline["feature_type"]
        feature_json["__label"] = ctx.label.raw_target()
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
        DefaultInfo(json_out),
    ]

_feature = rule(
    impl = _impl,
    attrs = {
        # feature targets are instances of `_feature` rules that are merged into
        # the output of this rule
        "feature_targets": attrs.list(
            attrs.dep(providers = [FeatureInfo]),
        ),
        # inline features are direct calls to a feature macro inside a layer()
        # or feature() rule instance
        "inline_features": attrs.dict(
            # Unique key for this feature (see _hash_key below)
            attrs.string(),
            attrs.dict(
                attrs.string(),
                attrs.any(),
            ),
        ),
        # Features need a way to coerce strings to sources or dependencies.
        # Map "feature key" -> "feature deps"
        "inline_features_deps": attrs.dict(attrs.string(), attrs.option(attrs.dict(attrs.string(), attrs.dep()))),
        # Map "feature key" -> "feature dep/source"
        "inline_features_deps_or_sources": attrs.dict(
            attrs.string(),
            attrs.dict(
                attrs.string(),
                attrs.one_of(attrs.dep(), attrs.source()),
            ),
        ),
    },
)

def feature(
        name: str.type,
        # No type hint here, but it is validated by flatten_features
        features,
        visibility = None):
    """
    Create a target representing a collection of one or more image features.

    `features` is a list that can contain either:
        - inline (aka unnamed) features created with macros like `install()`
        - labels referring to other `feature` targets
    """
    features = flatten.flatten(features, item_type = ["ParseTimeFeature", str.type, "selector"])
    inline_features = {}
    feature_targets = []
    inline_features_deps = {}
    inline_features_deps_or_sources = {}
    for feat in features:
        if types.is_string(feat):
            feature_targets.append(feat)
        elif type(feat) == "selector":
            # select() only works to choose between feature targets, not inline features
            feature_targets.append(feat)
        else:
            # type(feat) will show 'record' but we can assume its a ParseTimeFeature
            feature_key = _hash_key(feat)

            inline_features[feature_key] = {"feature_type": feat.feature_type, "kwargs": feat.kwargs}

            if feat.deps:
                # TODO: record providers for later checking
                inline_features_deps[feature_key] = {k: d.dep for k, d in feat.deps.items()}
            if feat.deps_or_sources:
                inline_features_deps_or_sources[feature_key] = feat.deps_or_sources

    return _feature(
        name = name,
        feature_targets = feature_targets,
        inline_features = inline_features,
        inline_features_deps = inline_features_deps,
        inline_features_deps_or_sources = inline_features_deps_or_sources,
        visibility = visibility,
    )

# We need a way to disambiguate inline features so that deps/sources can be
# passed back to them to convert to compiler json. This isn't persisted anywhere
# and does not end up in any target labels, so it does not need to be stable,
# just unique for a single evaluation of the target graph.
def _hash_key(x) -> str.type:
    return sha256(repr(x))

# Real, proper buck2 code can use this instead of the macro that shims some
# shitty buck1 conventions
feature_rule = _feature
