# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
DO NOT DEPEND ON THIS TARGET DIRECTLY, except through the `features=` field of
`feature.new` or `image.layer`.  A direct dependency will not work the way
you expect, and you will end up with incorrect behavior.

## Composing images using `feature.new`

When building regular binaries, one will often link multiple independent
libraries that know nothing about one another. Each of those libraries may
depend on other libraries, and so forth.

This ability to **compose** largely uncoupled pieces of functionality is an
essential tool of a software engineer.

`feature`s are a way of bringing the same sort of compositionality to
building filesystem images.

A feature specifies a set of **items**, each of which describes some aspect
**of a desired end state** for the filesystem.  Examples:
 - A directory must exist.
 - A tarball must be extracted at this location.
 - An RPM must be installed, or must be **ABSENT** from the filesystem.
 - Other `feature`s that must be installed.

Importantly, the specifications of `feature`s are not ordered. They are
not commands or instructions.  Rather, they are a declaration of what should be
true. You can think of a feature as a thunk or callback.

Note that you do **not** need `feature.new` to compose features within
a single project. Instead, avoid creating a Buck target and do this:

    feature_group1 = [f1, f2]
    feature_group2 = [f3, feature_group1, f4]
    image.layer(name = 'l', features = [feature_group2, f5])

In order to convert the declaration into action, one makes an `image.layer`.
Read that target's docblock for more info, but in essence, that will:
 - specify the initial state of the filesystem (aka the parent layer)
 - verify that the features can be applied correctly -- that dependencies are
   satisfied, that no features provide duplicate paths, etc.
 - install the features in dependency order,
 - capture the resulting filesystem, ready to be used as another parent layer.
"""

load("@bazel_skylib//lib:shell.bzl", "shell")
load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:constants.bzl", "BZL_CONST", "REPO_CFG")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")
load("//antlir/bzl:target_tagger.bzl", "extract_tagged_target", "new_target_tagger", "tag_target", "target_tagger_to_feature")
load("//antlir/bzl/image/feature:rpm_install_info_dummy_action_item.bzl", "RPM_INSTALL_INFO_DUMMY_ACTION_ITEM")

# ## Why are `feature`s forbidden as dependencies?
#
# The long target suffix below exists to discourage people from directly
# depending on `feature.new` targets.  They are not real targets, but rather a
# language feature to make it easy to compose independent features of container
# images.
#
# A normal Buck target is supposed to produce an output that completely
# encapsulates the outputs of all of its dependencies (think static linking),
# so in deciding whether to build a file or use a cached output, Buck will only
# consider direct dependencies, not transitive ones.
#
# In contrast, `feature.new` simply serializes its keyword arguments to JSON.
# It does not consume the outputs of its dependencies -- it reads neither
# regular target outputs, nor the JSONs of the `feature`s, on which it
# depends.
#
# By violating Buck semantics, `feature.new` creates two problems for
# targets that might depend on them:
#
# 1) Buck will build any target depending on an `feature` immediately
#    upon ensuring that its JSON output exists in the output tree.  It is
#    possible that the output tree lacks, or contains stale versions of, the
#    outputs of the targets, on which the `feature` itself depends.
#
# 2) If the output of a dependency of a `feature.new` target changes, this will
#    cause the feature to rebuild.  However, the output of the `feature.new`
#    will remain unchanged, and so any target depending on the `feature.new`
#    will **NOT** get rebuilt.
#
# For these reasons, special logic is required to correctly depend on
# `feature.new` targets.  At the moment, we are not aware of any reason to
# have direct access to the `feature.new` JSON outputs in any case.  Most
# users will want to depend on build artifacts that are downstream of
# `feature.new`, like `image.layer`.
#
# IMPORTANT: Keep in sync with `bzl_const.py`
def PRIVATE_DO_NOT_USE_feature_target_name(name):
    return name + BZL_CONST.PRIVATE_feature_suffix

def _flatten_nested_lists(lst):
    flat_lst = []

    # Manual recursion because Starlark doesn't allow real recursion
    stack = lst[:]
    max_int = 2147483647
    for step_counter in range(max_int):  # while True:
        if not stack:
            break
        if step_counter == max_int - 1:
            fail("Hit manual recursion limit")
        v = stack.pop()
        if types.is_list(v):
            for x in v:
                stack.append(x)
        else:
            flat_lst.append(v)
    return flat_lst

def _normalize_feature_and_get_deps(feature, flavors):
    "Returns a ready-to-serialize feature dictionary and its direct deps."
    target_tagger = new_target_tagger()

    feature_dict = {
        feature_key: [
            (
                shape.as_serializable_dict(feature) if
                # Some features have been converted to `shape`.  To make
                # them serializable together with legacy features, we must
                # turn these shapes into JSON-ready dicts.
                #
                # Specifically, this transformation removes the private
                # `__shape__` field, and asserts that there are no
                # `shape.target()` fields -- shapes are not integrated with
                # target_tagger yet, so one has to explicitly target-tag the
                # stuff that goes into these shapes.
                #
                # Future: once we shapify all features, this explicit
                # conversion can be removed since shape serialization will
                # just do the right thing.
                shape.is_any_instance(feature) else feature
            )
            for feature in features
        ]
        for feature_key, features in structs.to_dict(feature.items).items()
    }

    # For RPM actions, we must mutate the inner dicts of `feature_dict`
    # below.  As it turns out, `feature_dict` retains the same `dict`
    # instance that was created in `rpm_install`, which may well be reused
    # across multiple layers that use the same feature object in the same
    # project. To avoid aliasing bugs, copy all these dicts.
    aliased_rpms = feature_dict.get("rpms", [])
    if aliased_rpms:
        feature_dict["rpms"] = [dict(**r) for r in aliased_rpms]

    # Now that we know what flavors we need to build for the layer,
    # we can only include dependencies from the feature that are
    # necessary for the given flavor. This is needed to prevent
    # build errors from depending on non-existent rpms.
    deps = {d: 1 for d in feature.deps}
    for rpm_item in feature_dict.get("rpms", []):
        flavor_to_version_set = {}

        for flavor, version_set in rpm_item.get("flavor_to_version_set", {}).items():
            # If flavors are not provided, we are reading the flavor
            # from the parent layer, so we should include all possible flavors
            # for the rpm as the final flavor is not known until we are in python.
            if (
                not flavors and (
                    rpm_item.get("flavors_specified") or
                    flavor in REPO_CFG.stable_flavors
                )
            ) or (
                flavors and flavor in flavors
            ):
                flavor_to_version_set[flavor] = version_set
            elif version_set != BZL_CONST.version_set_allow_all_versions:
                target = extract_tagged_target(version_set)
                deps.pop(target)

        if not flavor_to_version_set and rpm_item["name"] != RPM_INSTALL_INFO_DUMMY_ACTION_ITEM:
            fail("Rpm `{}` must have one of the flavors `{}`".format(
                rpm_item["name"] or rpm_item["source"],
                flavors,
            ))
        rpm_item["flavor_to_version_set"] = flavor_to_version_set

    direct_deps = []
    direct_deps.extend(deps.keys())
    direct_deps.extend(target_tagger.targets.keys())
    return feature_dict, direct_deps

def normalize_features(
        porcelain_targets_or_structs,
        human_readable_target,
        flavors):
    porcelain_targets_or_structs = _flatten_nested_lists(
        porcelain_targets_or_structs,
    )
    targets = []
    inline_features = []
    direct_deps = []
    rpm_install_flavors = {}
    for f in porcelain_targets_or_structs:
        if types.is_string(f):
            targets.append(
                PRIVATE_DO_NOT_USE_feature_target_name(f),
            )
        else:
            feature_dict, more_deps = _normalize_feature_and_get_deps(
                feature = f,
                flavors = flavors,
            )

            valid_rpms = []
            for rpm_item in feature_dict.get("rpms", []):
                if rpm_item["action"] == "install":
                    for flavor, _ in rpm_item["flavor_to_version_set"].items():
                        rpm_install_flavors[flavor] = 1

                # We add a dummy in `_build_rpm_feature` in `rpms.bzl`
                # to hold information about the action and flavor for
                # empty rpm lists for validity checks.
                # See the comment in `_build_rpm_feature` for more
                # information.
                if rpm_item["name"] != RPM_INSTALL_INFO_DUMMY_ACTION_ITEM:
                    valid_rpms.append(rpm_item)

            if "rpms" in feature_dict:
                feature_dict["rpms"] = valid_rpms

            direct_deps.extend(more_deps)
            feature_dict["target"] = human_readable_target
            inline_features.append(feature_dict)

    # Skip coverage check for `antlir_test` since it's just for testing purposes and doesn't always
    # need to be covered.
    required_flavors = flavors or [flavor for flavor in REPO_CFG.stable_flavors if flavor != "antlir_test"]
    missing_flavors = [flavor for flavor in required_flavors if flavor not in rpm_install_flavors]
    if rpm_install_flavors and missing_flavors:
        fail("Missing `rpms_install` for flavors `{}`. Expected `{}`".format(missing_flavors, required_flavors))

    return struct(
        targets = targets,
        inline_features = inline_features,
        direct_deps = direct_deps,
    )

def private_do_not_use_feature_json_genrule(
        name,
        deps,
        output_feature_cmd,
        visibility):
    name = PRIVATE_DO_NOT_USE_feature_target_name(name)
    buck_genrule(
        name = name,
        type = "image_feature",  # For queries
        # Future: It'd be nice to refactor `bash.bzl` and to use the
        # log-on-error wrapper here (for `fetched_package_layer`).
        bash = """
        # {deps}
        set -ue -o pipefail
        {output_feature_cmd}
        """.format(
            deps = " ".join([
                "$(location {})".format(t)
                for t in sorted(deps)
            ]),
            output_feature_cmd = output_feature_cmd,
        ),
        visibility = visibility,
        antlir_rule = "user-internal",
    )

def private_feature_new(
        name,
        features,
        visibility = None,
        flavors = None,
        parent_layer = None):
    """
    Turns a group of image actions into a Buck target, so it can be
    referenced from outside the current project via `//path/to:name`.

    Do NOT use this for composition within one project, just use a list.

    See the file docblock for more details on image action composition.

    See other `.bzl` files in this directory for actions that actually build
    the container (install RPMs, remove files/directories, create symlinks
    or directories, copy executable or data files, declare mounts).
    """
    visibility = visibility or []

    # (1) Normalizes & annotates Buck target names so that they can be
    #     automatically enumerated from our JSON output.
    # (2) Builds a list of targets so that this converter can tell Buck
    #     that the `feature` target depends on it.
    target_tagger = new_target_tagger()

    # Omit the ugly suffix here since this is meant only for humans to read while debugging.
    # For inline targets, `image/layer/layer.bzl` sets this to the layer target path.
    human_readable_target = normalize_target(":" + name)
    normalized_features = normalize_features(
        features,
        human_readable_target,
        flavors,
    )

    feature = target_tagger_to_feature(
        target_tagger,
        items = struct(
            target = human_readable_target,
            features = [
                tag_target(target_tagger, t)
                for t in normalized_features.targets
            ] + normalized_features.inline_features,
        ),
        extra_deps = normalized_features.direct_deps,
    )

    # to mirror dependency on parent layer feature in buck2 logic
    if (BZL_CONST.layer_feature_suffix in name and parent_layer):
        feature.deps.append(
            PRIVATE_DO_NOT_USE_feature_target_name(parent_layer + BZL_CONST.layer_feature_suffix),
        )

    # Serialize the arguments and defer our computation until build-time.
    #
    # This allows us to automatically infer what is provided by RPMs & TARs,
    # and makes the implementation easier.
    #
    # Caveat: if the serialization exceeds the kernel's MAX_ARG_STRLEN,
    # this will fail (128KB on the Linux system I checked).
    #
    # TODO: Print friendlier error messages on user error.

    private_do_not_use_feature_json_genrule(
        name = name,
        deps = feature.deps,
        output_feature_cmd = 'echo {out} > "$OUT"'.format(
            out = shell.quote(structs.as_json(feature.items)),
        ),
        visibility = visibility,
    )

def feature_new(
        name,
        features,
        visibility = None,
        # This is used when a user wants to declare a feature
        # that is not available for all flavors in REPO_CFG.flavor_to_config.
        # An example of this is the internal feature in `image_layer.bzl`.
        flavors = None):
    private_feature_new(
        name,
        features,
        visibility,
        flavors,
    )
