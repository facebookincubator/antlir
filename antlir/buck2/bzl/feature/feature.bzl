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
load("//antlir/buck2/bzl:flavor.bzl", "FlavorInfo")
load("//antlir/bzl:constants.bzl", "BZL_CONST")
load("//antlir/bzl:flatten.bzl", "flatten")
load(":clone.bzl", "clone_to_json")
load(":ensure_dirs_exist.bzl", "ensure_dirs_exist_to_json")
load(":feature_info.bzl", "InlineFeatureInfo")
load(":genrule.bzl", "genrule_to_json")
load(":install.bzl", "install_to_json")
load(":meta_kv.bzl", "meta_remove_to_json", "meta_store_to_json")
load(":mount.bzl", "mount_to_json")
load(":parent_layer.bzl", "parent_layer_to_json")
load(":receive_sendstream.bzl", "receive_sendstream_to_json")
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
    "clone": clone_to_json,
    "ensure_dir_symlink": symlink_to_json,
    "ensure_dirs_exist": ensure_dirs_exist_to_json,
    "ensure_file_symlink": symlink_to_json,
    "genrule": genrule_to_json,
    "group": group_to_json,
    "install": install_to_json,
    "meta_key_value_remove": meta_remove_to_json,
    "meta_key_value_store": meta_store_to_json,
    "mount": mount_to_json,
    "parent_layer": parent_layer_to_json,
    "receive_sendstream": receive_sendstream_to_json,
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
        inline = InlineFeatureInfo(**json.decode(inline))
        feature_sources = ctx.attrs.inline_features_sources[key]
        feature_deps = ctx.attrs.inline_features_deps[key]
        if feature_sources:
            inline_deps.extend(feature_sources.values())
        if feature_deps:
            for dep in feature_deps.values():
                inline_deps.extend(dep[DefaultInfo].default_outputs)

        to_json_kwargs = inline.kwargs
        if feature_sources != None:
            to_json_kwargs["sources"] = feature_sources
        if feature_deps != None:
            to_json_kwargs["deps"] = feature_deps

        feature_json = _feature_to_json[inline.feature_type](**to_json_kwargs)
        feature_json["__feature_type"] = inline.feature_type
        feature_json["__label"] = ctx.label
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

    buck1_features_json = ctx.actions.declare_output("buck1/features.json")
    ctx.actions.run(
        cmd_args(
            ctx.attrs.translate_features[RunInfo],
            "--label=" + str(ctx.label),
            json_files.project_as_args("feature_json"),
            "--output",
            buck1_features_json.as_output(),
        ),
        category = "translate_features_to_buck1",
    )
    return [
        FeatureInfo(
            json_files = json_files,
            deps = deps,
        ),
        DefaultInfo(default_outputs = [json_out], sub_targets = {
            "buck1/features.json": [DefaultInfo(default_outputs = [buck1_features_json])],
        }),
    ]

_feature = rule(
    impl = _impl,
    attrs = {
        # feature targets are instances of `_feature` rules that are merged into
        # the output of this rule
        "feature_targets": attrs.list(
            attrs.dep(providers = [FeatureInfo]),
        ),
        "flavors": attrs.option(attrs.list(
            attrs.dep(providers = [FlavorInfo]),
        ), doc = "Restrict this feature to only layers that have one of these flavors", default = None),
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
        "inline_features_deps": attrs.dict(attrs.string(), attrs.option(attrs.dict(attrs.string(), attrs.dep()))),
        # Map "feature key" -> "feature sources"
        "inline_features_sources": attrs.dict(attrs.string(), attrs.option(attrs.dict(attrs.string(), attrs.source()))),
        "translate_features": attrs.default_only(attrs.exec_dep(default = "//antlir/buck2/translate_features:translate-features")),
    },
)

def feature(
        name: str.type,
        # No type hint here, but it is validated by flatten_features
        features,
        flavors = None,
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
            inline_features_deps[feature_key] = feat.deps
            inline_features_sources[feature_key] = feat.sources
            inline_features[feature_key] = json.encode(feat)

    # TODO(T139523690)
    native.alias(
        name = name + BZL_CONST.PRIVATE_feature_suffix,
        actual = ":" + name + "[buck1/features.json]",
        visibility = visibility,
    )

    return _feature(
        name = name,
        flavors = flavors,
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
