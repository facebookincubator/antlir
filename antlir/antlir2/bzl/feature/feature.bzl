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

Image features must also provide an anonymous rule implementation to convert the
kwargs, sources and deps into a JSON struct readable by the compiler. This
function must then be added to the `_anon_rules` map in this file.
"""

load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "cfg_attrs")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "MultiFeatureAnalysis")
load("//antlir/antlir2/features/clone:clone.bzl", "clone_rule")
load("//antlir/antlir2/features/dot_meta:dot_meta.bzl", "dot_meta_rule")
load("//antlir/antlir2/features/ensure_dir_exists:ensure_dir_exists.bzl", "ensure_dir_exists_rule")
load("//antlir/antlir2/features/extract:extract.bzl", "extract_buck_binary_rule", "extract_from_layer_rule")
# @oss-disable
# @oss-disable
# @oss-disable
# @oss-disable
# @oss-disable
# @oss-disable
load("//antlir/antlir2/features/genrule:genrule.bzl", "genrule_rule")
load("//antlir/antlir2/features/group:group.bzl", "group_rule")
load("//antlir/antlir2/features/install:install.bzl", "install_rule")
load("//antlir/antlir2/features/mount:mount.bzl", "mount_rule")
load("//antlir/antlir2/features/remove:remove.bzl", "remove_rule")
load("//antlir/antlir2/features/requires:requires.bzl", "requires_rule")
load("//antlir/antlir2/features/rpm:rpm.bzl", "rpms_record", "rpms_rule")
load("//antlir/antlir2/features/symlink:symlink.bzl", "ensure_dir_symlink_rule", "ensure_file_symlink_rule")
load("//antlir/antlir2/features/tarball:tarball.bzl", "tarball_rule")
load("//antlir/antlir2/features/test_only_features/trace:trace.bzl", "trace_rule")
load("//antlir/antlir2/features/user:user.bzl", "user_rule")
load("//antlir/antlir2/features/usermod:usermod.bzl", "usermod_rule")
load("//antlir/antlir2/os:cfg.bzl", "remove_os_transition")
load("//antlir/bzl:flatten.bzl", "flatten")
load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl:types.bzl", "types")
load("//antlir/bzl/build_defs.bzl", "config")
load(":cfg.bzl", "feature_cfg")

feature_record = record(
    feature_type = str,
    label = TargetLabel,
    analysis = FeatureAnalysis,
    plugin = FeaturePluginInfo | Provider,
)

def verify_feature_records(features: list[feature_record | typing.Any]) -> None:
    if (
        native.read_config("antlir", "strict-type-checks") == None and
        native.read_config("antlir", "strict-feature-record-type-checks") == None
    ):
        return

    def _assert_feature_record(_: feature_record):
        pass

    [_assert_feature_record(i) for i in features]  # buildifier: disable=no-effect

def feature_as_json(feat: feature_record | typing.Any) -> struct:
    verify_feature_records([feat])
    return struct(
        feature_type = feat.feature_type,
        label = feat.label,
        data = feat.analysis.data,
        plugin = feat.plugin,
    )

_anon_rules = {
    "clone": clone_rule,
    "dot_meta": dot_meta_rule,
    "ensure_dir_exists": ensure_dir_exists_rule,
    "ensure_dir_symlink": ensure_dir_symlink_rule,
    "ensure_file_symlink": ensure_file_symlink_rule,
    "extract_buck_binary": extract_buck_binary_rule,
    "extract_from_layer": extract_from_layer_rule,
    # @oss-disable
    # @oss-disable
    # @oss-disable
    # @oss-disable
    # @oss-disable
    "genrule": genrule_rule,
    "group": group_rule,
    "install": install_rule,
    "mount": mount_rule,
    "remove": remove_rule,
    "requires": requires_rule,
    "rpm": rpms_rule,
    "tarball": tarball_rule,
    "test_only_features/trace": trace_rule,
    "user": user_rule,
    "user_mod": usermod_rule,
}
# @oss-disable

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    # Merge inline features into a single JSON file
    inline_features = []
    anon_features = []
    feature_deps = []
    for feat in ctx.attrs.features:
        # select() can return None for some branches
        if not feat:
            continue
        if type(feat) == "dependency":
            feature_deps.append(feat)
            continue

        feature_type, plugin, kwargs, deps_or_srcs, srcs, deps, exec_deps, antlir2_configured_deps, unnamed_deps_or_srcs, args = feat

        anon_kwargs = kwargs | deps_or_srcs | srcs | deps | exec_deps | antlir2_configured_deps
        anon_kwargs["plugin"] = plugin

        # TODO: make args consistent with the other types
        if args:
            anon_kwargs["args"] = args
        if unnamed_deps_or_srcs:
            anon_kwargs["unnamed_deps_or_srcs"] = unnamed_deps_or_srcs

        anon_features.append(ctx.actions.anon_target(
            _anon_rules[feature_type],
            anon_kwargs,
        ))

    def _with_anon_features(anon_features: list[ProviderCollection]) -> list[Provider]:
        flat = []
        for af in anon_features:
            if FeatureAnalysis in af:
                flat.append(af[FeatureAnalysis])
            else:
                flat.extend(af[MultiFeatureAnalysis].features)

        anon_features = [
            feature_record(
                feature_type = af.feature_type,
                label = ctx.label.raw_target(),
                analysis = af,
                plugin = af.plugin,
            )
            for af in flat
        ]
        features = anon_features + inline_features
        for dep in feature_deps:
            features.extend(dep[FeatureInfo].features)
        features_json = [feature_as_json(f) for f in features]

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
    "features": attrs.list(
        # allow None to be intermixed in the features list so that a `select` is
        # able to do nothing for certain configurations
        attrs.option(
            attrs.one_of(
                attrs.dep(providers = [FeatureInfo], doc = "feature targets to include"),
                attrs.tuple(
                    attrs.string(doc = "ParseTimeFeature.feature_type"),
                    attrs.exec_dep(
                        providers = [FeaturePluginInfo],
                        doc = "ParseTimeFeature.plugin",
                    ),
                    attrs.dict(attrs.string(), _nestable_value, doc = "kwargs"),
                    attrs.dict(
                        attrs.string(),
                        attrs.one_of(
                            attrs.transition_dep(cfg = remove_os_transition),
                            # Need a non-transition dep to fallback on since
                            # this is also used in anonymous targets. The
                            # transition_dep must be first so that it's used at
                            # the top-level for correct configuration.
                            attrs.dep(),
                            attrs.source(),
                        ),
                        doc = "ParseTimeFeature.deps_or_srcs",
                    ),
                    attrs.dict(
                        attrs.string(),
                        attrs.source(),
                        doc = "ParseTimeFeature.srcs",
                    ),
                    attrs.dict(
                        attrs.string(),
                        attrs.one_of(
                            attrs.dep(),
                            # @oss-disable
                        ),
                        doc = "ParseTimeFeature.deps",
                    ),
                    attrs.dict(
                        attrs.string(),
                        attrs.exec_dep(),
                        doc = "ParseTimeFeature.exec_deps",
                    ),
                    attrs.dict(
                        attrs.string(),
                        attrs.dep(),
                        doc = "ParseTimeFeature.antlir2_configured_deps",
                    ),
                    attrs.list(
                        attrs.one_of(attrs.dep(), attrs.source()),
                        doc = "ParseTimeFeature.unnamed_deps_or_srcs",
                    ),
                    attrs.dict(
                        attrs.string(),
                        attrs.arg(anon_target_compatible = True),
                        doc = "ParseTimeFeature.args",
                    ),
                    doc = "inline feature definition",
                ),
            ),
            default = None,
        ),
        default = [],
    ),
    "labels": attrs.list(attrs.string(), default = []),
}

feature_rule = rule(
    impl = _impl,
    attrs = shared_features_attrs | cfg_attrs(),
    cfg = feature_cfg,
)

def feature_attrs(features) -> dict[str, typing.Any]:
    """
    Create a dict suitable to pass to the _feature rule.

    Used by both the feature() macro below and by anything wishing to create an
    anon_target doing all the feature analysis

    `features` is a list that can contain either:
        - inline (aka unnamed) features created with macros like `install()`
        - labels referring to other `feature` targets
    """
    if types.is_list(features):
        features = flatten.flatten(features)

    return {
        "features": features,
    }

def feature(
        name: str,
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
        **kwargs
    )

def regroup_features(label: Label, features: list[feature_record | typing.Any]) -> list[feature_record | typing.Any]:
    """
    Some features must be grouped at buck time so that the compiler can act on
    them as a single unit for performance reasons.

    Currently this is only true of the RPM features, but will likely be true of
    any future package managers as well.
    """
    ungrouped_features = []
    verify_feature_records(features)

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
