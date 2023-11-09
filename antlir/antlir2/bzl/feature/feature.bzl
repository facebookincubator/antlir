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
    - deps_or_srcs: map of key -> source for
        `attrs.one_of(attrs.dep(), attrs.source())` dependencies needed by the
        feature. The feature is always able to get the "artifact", and will be
        able to get provider details on "dependency" deps
    - deps: map of key -> dep for `attrs.dep()` dependencies needed by the
        feature.
    - kwargs: map of all non-dependency inputs
For `deps_or_srcs` and `deps`, the user input to the inline feature input will
just be a simple string that is a label (or path for plain source files), but by
including it in the special maps in `ParseTimeFeature`, the `feature` rule is
able to coerce those labels to concrete artifacts.

Image features must also provide a function to convert the kwargs, sources and
deps into a JSON struct readable by the compiler. This function must then be
added to the `_analyze_feature` map in this file.
"""

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo")
# @oss-disable
# @oss-disable
# @oss-disable
# @oss-disable
# @oss-disable
# @oss-disable
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/antlir2/features/test_only_features:trace.bzl", "trace_analyze")
load("//antlir/bzl:flatten.bzl", "flatten")
load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl/build_defs.bzl", "config")
load(":antlir1_no_equivalent.bzl", "antlir1_no_equivalent_analyze")
load(":cfg.bzl", "feature_cfg")
load(":clone.bzl", "clone_analyze")
load(":dot_meta.bzl", "dot_meta_analyze")
load(":ensure_dirs_exist.bzl", "ensure_dir_exists_analyze")
load(":extract.bzl", "extract_analyze")
load(":feature_info.bzl", "AnalyzeFeatureContext", "FeatureAnalysis", "Tools")
load(":genrule.bzl", "genrule_analyze")
load(":install.bzl", "install_analyze")
load(":mount.bzl", "mount_analyze")
load(":remove.bzl", "remove_analyze")
load(":requires.bzl", "requires_analyze")
load(":rpms.bzl", "rpms_analyze", "rpms_record")
load(":symlink.bzl", "ensure_dir_symlink_analyze", "ensure_file_symlink_analyze")
load(":tarball.bzl", "tarball_analyze")
load(":usergroup.bzl", "group_analyze", "user_analyze", "usermod_analyze")

feature_record = record(
    feature_type = str,
    label = TargetLabel,
    analysis = "FeatureAnalysis",
    plugin = FeaturePluginInfo | Provider,
)

def _feature_as_json(feat: feature_record) -> struct:
    return struct(
        feature_type = feat.feature_type,
        label = feat.label,
        data = feat.analysis.data,
        plugin = feat.plugin,
    )

_analyze_feature = {
    "antlir1_no_equivalent": antlir1_no_equivalent_analyze,
    "clone": clone_analyze,
    "dot_meta": dot_meta_analyze,
    "ensure_dir_exists": ensure_dir_exists_analyze,
    "ensure_dir_symlink": ensure_dir_symlink_analyze,
    "ensure_file_symlink": ensure_file_symlink_analyze,
    "extract": extract_analyze,
    # @oss-disable
    # @oss-disable
    # @oss-disable
    # @oss-disable
    # @oss-disable
    "genrule": genrule_analyze,
    "group": group_analyze,
    "install": install_analyze,
    "mount": mount_analyze,
    "remove": remove_analyze,
    "requires": requires_analyze,
    "rpm": rpms_analyze,
    "tarball": tarball_analyze,
    "test_only_features/trace": trace_analyze,
    "user": user_analyze,
    "user_mod": usermod_analyze,
}
# @oss-disable

_anon_rules = {
}

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    # Merge inline features into a single JSON file
    inline_features = []
    inline_artifacts = []
    inline_run_infos = []
    inline_layer_deps = []
    anon_features = []
    for key, inline in ctx.attrs.inline_features.items():
        feature_deps = ctx.attrs.inline_features_deps.get(key, None)
        feature_deps_or_srcs = ctx.attrs.inline_features_deps_or_srcs.get(key, None)
        feature_unnamed_deps_or_srcs = ctx.attrs.inline_features_unnamed_deps_or_srcs.get(key, None)
        feature_exec_deps = ctx.attrs.inline_features_exec_deps.get(key, None)
        feature_srcs = ctx.attrs.inline_features_srcs.get(key, None)
        feature_args = ctx.attrs.inline_features_args.get(key, None)

        analyze_kwargs = inline["kwargs"]
        if feature_deps != None:
            analyze_kwargs["deps"] = feature_deps
        if feature_deps_or_srcs != None:
            analyze_kwargs["deps_or_srcs"] = feature_deps_or_srcs
        if feature_unnamed_deps_or_srcs != None:
            analyze_kwargs["unnamed_deps_or_srcs"] = feature_unnamed_deps_or_srcs
        if feature_exec_deps != None:
            analyze_kwargs["exec_deps"] = feature_exec_deps
        if feature_srcs != None:
            analyze_kwargs["srcs"] = feature_srcs
        if feature_args != None:
            analyze_kwargs["args"] = feature_args
        if inline.get("analyze_uses_context"):
            analyze_kwargs["ctx"] = AnalyzeFeatureContext(
                unique_action_identifier = key,
                actions = ctx.actions,
                tools = Tools(
                    objcopy = ctx.attrs._objcopy,
                ),
                label = ctx.label,
            )
        analyze_kwargs["plugin"] = ctx.attrs.inline_features_plugins[key][FeaturePluginInfo]

        if inline["feature_type"] in _analyze_feature:
            analysis = _analyze_feature[inline["feature_type"]](**analyze_kwargs)
            for analysis in flatten.flatten(analysis):
                inline_artifacts.extend(analysis.required_artifacts)
                inline_run_infos.extend(analysis.required_run_infos)
                inline_layer_deps.extend(analysis.required_layers)

                feat = feature_record(
                    feature_type = analysis.feature_type,
                    label = ctx.label.raw_target(),
                    analysis = analysis,
                    plugin = analysis.plugin or ctx.attrs.inline_features_plugins[key][FeaturePluginInfo],
                )
                inline_features.append(feat)
        else:
            anon_args = analyze_kwargs
            anon_args["plugin"] = ctx.attrs.inline_features_plugins[key]
            anon_features.append(ctx.actions.anon_target(
                _anon_rules[inline["feature_type"]],
                analyze_kwargs,
            ))

    def _with_anon_features(anon_features: list[ProviderCollection]) -> list[Provider]:
        anon_features = [
            feature_record(
                feature_type = af[FeatureAnalysis].feature_type,
                label = ctx.label.raw_target(),
                analysis = af[FeatureAnalysis],
                plugin = af[FeatureAnalysis].plugin,
            )
            for af in anon_features
        ]
        features = anon_features + inline_features
        for dep in ctx.attrs.feature_targets:
            # select() can return None for some branches
            if not dep:
                continue
            deps = flatten.flatten(dep)
            for dep in deps:
                features.extend(dep[FeatureInfo].features)
        features_json = [_feature_as_json(f) for f in features]

        json_file = ctx.actions.write_json("features.json", features_json)

        return [
            FeatureInfo(
                features = features,
            ),
            DefaultInfo(json_file),
        ]

    if anon_features:
        return anon_features[0].promise.join(*[af.promise for af in anon_features[1:]]).map(_with_anon_features)
    else:
        return _with_anon_features([])

# This horrible set of pseudo-exhaustive `one_of` calls is because there
# currently is nothing like `attrs.json()` that will force things like `select`
# to be coerced to real concrete values.
# This nesting _can_ be extended if features grow more complicated kwargs, but
# that's unlikely, so I'm stopping here for now
# https://fb.workplace.com/groups/347532827186692/posts/632399858699986
_primitive = attrs.option(attrs.one_of(attrs.string(), attrs.int(), attrs.bool()))
_value = attrs.one_of(
    _primitive,
    attrs.dict(_primitive, _primitive),
    attrs.list(_primitive),
)
_nestable_value = attrs.one_of(
    _value,
    attrs.dict(_primitive, _value),
    attrs.dict(_primitive, attrs.dict(_primitive, _value)),
    attrs.dict(_primitive, attrs.list(_value)),
    attrs.list(_value),
    attrs.list(attrs.dict(_primitive, _value)),
    attrs.list(attrs.list(_value)),
)
nestable_value = _nestable_value

shared_features_attrs = {
    # feature targets are instances of `_feature` rules that are merged into
    # the output of this rule
    "feature_targets": attrs.list(
        attrs.one_of(
            # optional so that a `select` can return `None` for some configurations
            attrs.option(
                attrs.dep(providers = [FeatureInfo]),
            ),
            attrs.list(attrs.dep(providers = [FeatureInfo])),
        ),
        default = [],
    ),
    # inline features are direct calls to a feature macro inside a layer()
    # or feature() rule instance
    "inline_features": attrs.dict(
        # Unique key for this feature (see _hash_key below)
        attrs.string(),
        attrs.dict(
            # top level kwargs
            attrs.string(),  # kwarg name
            _nestable_value,
        ),
        default = {},
    ),
    # Map "feature key" -> "feature attrs.arg"
    "inline_features_args": attrs.dict(
        attrs.string(),
        attrs.option(attrs.dict(attrs.string(), attrs.arg(anon_target_compatible = True))),
        default = {},
    ),
    # Features need a way to coerce strings to sources or dependencies.
    # Map "feature key" -> "feature deps"
    "inline_features_deps": attrs.dict(
        attrs.string(),
        attrs.option(
            attrs.dict(
                attrs.string(),
                attrs.one_of(
                    attrs.dep(),
                    # @oss-disable
                ),
            ),
        ),
        default = {},
    ),
    # Map "feature key" -> "feature dep/source"
    "inline_features_deps_or_srcs": attrs.dict(
        attrs.string(),
        attrs.dict(
            attrs.string(),
            attrs.one_of(attrs.dep(), attrs.source()),
        ),
        default = {},
    ),
    # Map "feature key" -> "feature exec_dep"
    "inline_features_exec_deps": attrs.dict(
        attrs.string(),
        attrs.option(attrs.dict(attrs.string(), attrs.exec_dep())),
        default = {},
    ),
    # Map "feature key" -> "feature impl binary"
    "inline_features_plugins": attrs.dict(
        attrs.string(),
        attrs.exec_dep(providers = [FeaturePluginInfo]),
        default = {},
    ),
    # Map "feature key" -> "feature srcs"
    "inline_features_srcs": attrs.dict(
        attrs.string(),
        attrs.option(attrs.dict(attrs.string(), attrs.source())),
        default = {},
    ),
    # Map "feature key" -> "feature dep/source"
    "inline_features_unnamed_deps_or_srcs": attrs.dict(
        attrs.string(),
        attrs.list(
            attrs.one_of(attrs.dep(), attrs.source()),
        ),
        default = {},
    ),
}

_features_attrs = {
    "_objcopy": attrs.exec_dep(default = "fbsource//third-party/binutils:objcopy"),
}

_features_attrs.update(shared_features_attrs)

feature_rule = rule(
    impl = _impl,
    attrs = _features_attrs,
    cfg = feature_cfg,
)

def feature_attrs(
        # No type hint here, but it is validated by flatten_features
        features) -> dict[str, typing.Any]:
    """
    Create a dict suitable to pass to the _feature rule.

    Used by both the feature() macro below and by anything wishing to create an
    anon_target doing all the feature analysis

    `features` is a list that can contain either:
        - inline (aka unnamed) features created with macros like `install()`
        - labels referring to other `feature` targets
    """
    features = flatten.flatten(features)

    # Some antlir1 features may have crept in here because it's hard to refactor
    # tons of bzl at once, so if it has a .antlir2_feature attribute, we'll be
    # nice and allow it
    features = [f.antlir2_feature if hasattr(f, "antlir2_feature") else f for f in features]

    # This is already flat but this will enforce the type checking again after
    # the antlir1-compat-promotion above
    features = flatten.flatten(features, item_type = ["ParseTimeFeature", str, "selector"])

    inline_features = {}
    feature_targets = []
    inline_features_plugins = {}
    inline_features_deps = {}
    inline_features_deps_or_srcs = {}
    inline_features_srcs = {}
    inline_features_exec_deps = {}
    inline_features_unnamed_deps_or_srcs = {}
    inline_features_args = {}
    features_target_compatible_with = []
    for feat in features:
        if types.is_string(feat):
            feature_targets.append(feat)
        elif type(feat) == "selector":
            # select() only works to choose between feature targets, not inline features
            feature_targets.append(feat)
        else:
            # type(feat) will show 'record' but we can assume its a ParseTimeFeature
            feature_key = _hash_key(feat)

            inline_features[feature_key] = {
                "analyze_uses_context": feat.analyze_uses_context,
                "feature_type": feat.feature_type,
                "kwargs": feat.kwargs,
            }

            inline_features_plugins[feature_key] = feat.plugin

            if feat.deps:
                # TODO: record providers for later checking
                inline_features_deps[feature_key] = {k: d.dep for k, d in feat.deps.items()}
            if feat.exec_deps:
                inline_features_exec_deps[feature_key] = {k: d.dep for k, d in feat.exec_deps.items()}
            if feat.deps_or_srcs:
                inline_features_deps_or_srcs[feature_key] = feat.deps_or_srcs
            if feat.unnamed_deps_or_srcs:
                inline_features_unnamed_deps_or_srcs[feature_key] = feat.unnamed_deps_or_srcs
            if feat.args:
                inline_features_args[feature_key] = feat.args
            if feat.srcs:
                inline_features_srcs[feature_key] = feat.srcs
            if feat.target_compatible_with:
                features_target_compatible_with.extend(feat.target_compatible_with)

    return {
        "feature_targets": feature_targets,
        "inline_features": inline_features,
        "inline_features_args": inline_features_args,
        "inline_features_deps": inline_features_deps,
        "inline_features_deps_or_srcs": inline_features_deps_or_srcs,
        "inline_features_exec_deps": inline_features_exec_deps,
        "inline_features_plugins": inline_features_plugins,
        "inline_features_srcs": inline_features_srcs,
        "inline_features_unnamed_deps_or_srcs": inline_features_unnamed_deps_or_srcs,
        "target_compatible_with": features_target_compatible_with,
    }

def feature(
        name: str,
        # No type hint here, but it is validated by flatten_features
        features,
        visibility = None,
        **kwargs):
    """
    Create a target representing a collection of one or more image features.

    `features` is a list that can contain either:
        - inline (aka unnamed) features created with macros like `install()`
        - labels referring to other `feature` targets
    """
    attrs = feature_attrs(features)
    kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform

    kwargs.update(attrs)

    return feature_rule(
        name = name,
        visibility = visibility,
        **attrs
    )

# We need a way to disambiguate inline features so that deps/sources can be
# passed back to them to convert to compiler json. This isn't persisted anywhere
# and does not end up in any target labels, so it does not need to be stable,
# just unique for a single evaluation of the target graph.
def _hash_key(x) -> str:
    return sha256(repr(x))

def regroup_features(label: Label, features: list[feature_record]) -> list[feature_record]:
    """
    Some features must be grouped at buck time so that the compiler can act on
    them as a single unit for performance reasons.

    Currently this is only true of the RPM features, but will likely be true of
    any future package managers as well.
    """
    ungrouped_features = []

    # keep all the extra junk (like run_info) attached to the rpm feature
    any_rpm_feature = None
    rpm_required_artifacts = []
    rpm_items = []
    for feat in features:
        if feat.feature_type == "rpm":
            rpm_items.extend(feat.analysis.data.items)
            rpm_required_artifacts.extend(feat.analysis.required_artifacts)
            any_rpm_feature = feat
        else:
            ungrouped_features.append(feat)

    if any_rpm_feature:
        # records are immutable, so we have to do this crazy dance to just
        # mutate the rpm feature data
        rpm_feature = structs.to_dict(any_rpm_feature)
        rpm_feature["analysis"] = structs.to_dict(rpm_feature["analysis"])
        rpm_feature["analysis"]["data"] = structs.to_dict(rpm_feature["analysis"]["data"])
        rpm_feature["analysis"]["data"]["items"] = rpm_items
        rpm_feature["analysis"]["data"] = rpms_record(**rpm_feature["analysis"]["data"])
        rpm_feature["analysis"]["required_artifacts"] = rpm_required_artifacts
        rpm_feature["analysis"] = FeatureAnalysis(**rpm_feature["analysis"])
        rpm_feature["label"] = label.raw_target()
        rpm_feature = feature_record(**rpm_feature)
    else:
        rpm_feature = None

    return ungrouped_features + ([rpm_feature] if rpm_feature else [])
