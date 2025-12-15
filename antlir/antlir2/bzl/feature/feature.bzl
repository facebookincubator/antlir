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
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginPluginKind")
load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "MultiFeatureAnalysis", "feature_record")
load("//antlir/antlir2/features/clone:clone.bzl", "clone_rule")
load("//antlir/antlir2/features/dot_meta:dot_meta.bzl", "dot_meta_rule")
load("//antlir/antlir2/features/ensure_dir_exists:ensure_dir_exists.bzl", "ensure_dir_exists_rule")
load("//antlir/antlir2/features/extract:extract.bzl", "extract_buck_binary_rule", "extract_from_layer_rule")
# @oss-disable[end= ]: load("//antlir/antlir2/features/facebook/chef_solo:chef_solo.bzl", "chef_solo_rule")
# @oss-disable[end= ]: load("//antlir/antlir2/features/facebook/chef_solo/json_recipes:json_recipes.bzl", "json_recipes_rule")
# @oss-disable[end= ]: load("//antlir/antlir2/features/facebook/container:rules.bzl", "tw_container_anon_rules")
# @oss-disable[end= ]: load("//antlir/antlir2/features/facebook/fbpkg_builder:fbpkg_builder.bzl", "fbpkg_builder_rule", "path_actions_attrs_t")
# @oss-disable[end= ]: load("//antlir/antlir2/features/facebook/fbpkg_install:fbpkg_install.bzl", "fbpkg_install_rule")
# @oss-disable[end= ]: load("//antlir/antlir2/features/facebook/fetch_fbpkg_mount:fetch_fbpkg_mount.bzl", "fetch_fbpkg_mount_rule")
load("//antlir/antlir2/features/genrule:genrule.bzl", "genrule_rule")
load("//antlir/antlir2/features/group:group.bzl", "group_rule")
load("//antlir/antlir2/features/hardlink:hardlink.bzl", "hardlink_rule")
load("//antlir/antlir2/features/install:install.bzl", "install_rule")
load("//antlir/antlir2/features/mount:mount.bzl", "mount_rule")
load("//antlir/antlir2/features/remove:remove.bzl", "remove_rule")
load("//antlir/antlir2/features/requires:requires.bzl", "requires_rule")
load("//antlir/antlir2/features/rpm:rpm.bzl", "rpms_rule")
load("//antlir/antlir2/features/symlink:symlink.bzl", "ensure_dir_symlink_rule", "ensure_file_symlink_rule")
load("//antlir/antlir2/features/tarball:tarball.bzl", "tarball_rule")
load("//antlir/antlir2/features/test_only_features/extend_facts:extend_facts.bzl", "extend_facts_rule")
load("//antlir/antlir2/features/test_only_features/trace:trace.bzl", "trace_rule")
load("//antlir/antlir2/features/user:user.bzl", "user_rule")
load("//antlir/antlir2/features/usermod:usermod.bzl", "usermod_rule")
load("//antlir/bzl:build_defs.bzl", "config")
load("//antlir/bzl:flatten.bzl", "flatten")
load("//antlir/bzl:types.bzl", "types")
load(":cfg.bzl", "feature_cfg")

_anon_rules = {
    "clone": clone_rule,
    "dot_meta": dot_meta_rule,
    "ensure_dir_exists": ensure_dir_exists_rule,
    "ensure_dir_symlink": ensure_dir_symlink_rule,
    "ensure_file_symlink": ensure_file_symlink_rule,
    "extract_buck_binary": extract_buck_binary_rule,
    "extract_from_layer": extract_from_layer_rule,
    # @oss-disable[end= ]: "facebook/chef_solo": chef_solo_rule,
    # @oss-disable[end= ]: "facebook/chef_solo/json_recipes": json_recipes_rule,
    # @oss-disable[end= ]: "facebook/fbpkg_builder": fbpkg_builder_rule,
    # @oss-disable[end= ]: "facebook/fbpkg_install": fbpkg_install_rule,
    # @oss-disable[end= ]: "facebook/fetch_fbpkg_mount": fetch_fbpkg_mount_rule,
    "genrule": genrule_rule,
    "group": group_rule,
    "hardlink": hardlink_rule,
    "install": install_rule,
    "mount": mount_rule,
    "remove": remove_rule,
    "requires": requires_rule,
    "rpm": rpms_rule,
    "tarball": tarball_rule,
    "test_only_features/extend_facts": extend_facts_rule,
    "test_only_features/trace": trace_rule,
    "user": user_rule,
    "user_mod": usermod_rule,
}
# @oss-disable[end= ]: _anon_rules.update(tw_container_anon_rules)

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    # Merge inline features into a single JSON file
    inline_features = []
    anon_features = []
    feature_deps = []
    for feat in flatten.flatten(ctx.attrs.features):
        # select() can return None for some branches
        if not feat:
            continue
        if isinstance(feat, Dependency):
            feature_deps.append(feat)
            continue

        feature_type, plugin, uses_plugins, kwargs, deps_or_srcs, srcs, deps, exec_deps, distro_platform_deps, unnamed_deps_or_srcs, args = feat

        anon_kwargs = uses_plugins | kwargs | deps_or_srcs | srcs | deps | exec_deps | distro_platform_deps
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

        json_file = ctx.actions.write_json(
            "features.json",
            [as_json_for_depgraph(feature) for feature in features],
        )

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

# allow None to be intermixed in the features list so that a `select` is
# able to do nothing for certain configurations
_nested_feature_type = attrs.option(
    attrs.one_of(
        attrs.dep(
            providers = [FeatureInfo],
            pulls_and_pushes_plugins = [FeaturePluginPluginKind],
            doc = "feature targets to include",
        ),
        attrs.tuple(
            attrs.string(doc = "ParseTimeFeature.feature_type"),
            attrs.one_of(
                attrs.plugin_dep(
                    kind = FeaturePluginPluginKind,
                    doc = "ParseTimeFeature.plugin",
                ),
                attrs.label(),
            ),
            attrs.dict(
                attrs.string(),
                attrs.one_of(
                    attrs.plugin_dep(
                        kind = FeaturePluginPluginKind,
                        doc = "ParseTimeFeature.uses_plugins",
                    ),
                    attrs.label(),
                ),
            ),
            attrs.dict(attrs.string(), _nestable_value, doc = "kwargs"),
            attrs.dict(
                attrs.string(),
                attrs.one_of(
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
                    attrs.list(attrs.dep()),
                    # @oss-disable[end= ]: path_actions_attrs_t,
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
                attrs.one_of(
                    attrs.transition_dep(cfg = "antlir//antlir/distro/transition:to-current-distro-platform"),
                    attrs.dep(),
                ),
                doc = "ParseTimeFeature.distro_platform_deps",
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
)

shared_features_attrs = {
    "features": attrs.list(
        attrs.one_of(
            _nested_feature_type,
            attrs.list(_nested_feature_type),
        ),
        default = [],
    ),
    "labels": attrs.list(attrs.string(), default = []),
}

feature_rule = rule(
    impl = _impl,
    attrs = shared_features_attrs | cfg_attrs(),
    cfg = feature_cfg,
    uses_plugins = [FeaturePluginPluginKind],
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
    if "default_target_platform" not in kwargs:
        kwargs["default_target_platform"] = config.get_platform_for_current_buildfile().target_platform

    kwargs.update(attrs)

    # TODO(T224478114) this really should not be necessary, but it currently is
    # since feature preserves its `exec_dep` plugins and downstream consumers
    # (other features and layer rules) use it, blindly assuming that the
    # execution platform for both targets is the same, even though there is
    # nothing that guarantees that.
    # As a quick workaround, just require that the feature execution happens on
    # aarch64 if the build host is aarch64, otherwise let it float
    if native.host_info().arch.is_aarch64:
        kwargs.setdefault("exec_compatible_with", ["ovr_config//cpu:arm64"])

    return feature_rule(
        name = name,
        visibility = visibility,
        **kwargs
    )

def reduce_features(features: list[feature_record | typing.Any]) -> list[feature_record | typing.Any]:
    """
    Some features must be reduced to one instance at buck time so that the
    compiler can act on them as a single unit for performance reasons.

    Currently this is only true of the RPM features, but will likely be true of
    any future package managers as well.
    """
    output = []

    reducing = {}

    for feature in features:
        if feature.analysis.reduce_fn:
            if feature.feature_type not in reducing:
                reducing[feature.feature_type] = feature
            else:
                reducing[feature.feature_type] = feature.analysis.reduce_fn(
                    reducing[feature.feature_type],
                    feature,
                )
        else:
            output.append(feature)

    return output + reducing.values()

def as_json_for_depgraph(feature: feature_record | typing.Any) -> struct:
    return struct(
        # serializing feature.analysis as a whole would cause tons of
        # unnecessary inputs to be materialized, so only analysis.data
        # is included
        data = feature.analysis.data,
        feature_type = feature.feature_type,
        label = feature.label,
        # this is a ProvidersLabel that needs explicit conversion to a string
        plugin = str(feature.plugin),
    )
