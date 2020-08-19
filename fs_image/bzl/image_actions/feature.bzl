"""
DO NOT DEPEND ON THIS TARGET DIRECTLY, except through the `features=` field of
`image.feature` or `image.layer`.  A direct dependency will not work the way
you expect, and you will end up with incorrect behavior.

## Composing images using `image.feature`

When building regular binaries, one will often link multiple independent
libraries that know nothing about one another. Each of those libraries may
depend on other libraries, and so forth.

This ability to **compose** largely uncoupled pieces of functionality is an
essential tool of a software engineer.

`image.feature` is a way of bringing the same sort of compositionality to
building filesystem images.

A feature specifies a set of **items**, each of which describes some aspect
**of a desired end state** for the filesystem.  Examples:
 - A directory must exist.
 - A tarball must be extracted at this location.
 - An RPM must be installed, or must be **ABSENT** from the filesystem.
 - Other `image.feature` that must be installed.

Importantly, the specifications of an `image.feature` are not ordered. They are
not commands or instructions.  Rather, they are a declaration of what should be
true. You can think of a feature as a thunk or callback.

Note that you do **not** need `image.feature` to compose features within
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
load("//fs_image/bzl:constants.bzl", "VERSION_SET_TO_PATH")
load("//fs_image/bzl:oss_shim.bzl", "buck_genrule", "get_visibility")
load("//fs_image/bzl:structs.bzl", "structs")
load("//fs_image/bzl:target_helpers.bzl", "normalize_target")
load("//fs_image/bzl:target_tagger.bzl", "new_target_tagger", "tag_target", "target_tagger_to_feature")

# ## Why are `image.feature`s forbidden as dependencies?
#
# The long target suffix below exists to discourage people from directly
# depending on `image.feature`s.  They are not real targets, but rather a
# language feature to make it easy to compose independent features of container
# images.
#
# A normal Buck target is supposed to produce an output that completely
# encapsulates the outputs of all of its dependencies (think static linking),
# so in deciding whether to build a file or use a cached output, Buck will only
# consider direct dependencies, not transitive ones.
#
# In contrast, `image.feature` simply serializes its keyword arguments to JSON.
# It does not consume the outputs of its dependencies -- it reads neither
# regular target outputs, nor the JSONs of the `image_feature`s, on which it
# depends.
#
# By violating Buck semantics, `image_features` creates two problems for
# targets that might depend on them:
#
# 1) Buck will build any target depending on an `image_feature` immediately
#    upon ensuring that its JSON output exists in the output tree.  It is
#    possible that the output tree lacks, or contains stale versions of, the
#    outputs of the targets, on which the `image_feature` itself depends.
#
# 2) If the output of a dependency of an `image.feature` changes, this will
#    cause the feature to rebuild.  However, the output of the `image.feature`
#    will remain unchanged, and so any target depending on the `image.feature`
#    will **NOT** get rebuilt.
#
# For these reasons, special logic is required to correctly depend on
# `image.feature` targets.  At the moment, we are not aware of any reason to
# have direct access to the `image.feature` JSON outputs in any case.  Most
# users will want to depend on build artifacts that are downstream of
# `image.feature`, like `image.layer`.
#
# Maintainers of this code: please change this string at will, **without**
# searching the codebase for people who might be referring to it.  They have
# seen this note, and they have agreed to have their code broken without
# warning.  Do not incentivize hacky engineering practices by "being nice."
# (Caveat: don't change it daily to avoid forcing excessive rebuilds.)
def PRIVATE_DO_NOT_USE_feature_target_name(name, version_set):
    name += "_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN"
    if version_set not in VERSION_SET_TO_PATH:
        fail("Must be in {}".format(list(VERSION_SET_TO_PATH)), "version_set")

    # When a feature is declared, it doesn't know the version set of the
    # layer that will use it, so we normally declare all possible variants.
    # This is only None when there are no version sets in use.
    if version_set != None:
        name += "__rpm_verset__" + version_set
    return name

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

def _normalize_feature_and_get_deps(feature, version_set):
    "Returns a ready-to-serialize feature dictionary and its direct deps."
    target_tagger = new_target_tagger()
    feature_dict = structs.to_dict(feature.items)

    # For RPM actions, we must mutate the inner dicts of `feature_dict`
    # below.  As it turns out, `feature_dict` retains the same `dict`
    # instance that was created in `rpm_install`, which may well be reused
    # across multiple layers that use the same feature object in the same
    # project. To avoid aliasing bugs, copy all these dicts.
    aliased_rpms = feature_dict.get("rpms", [])
    if aliased_rpms:
        # IMPORTANT: This is NOT a deep copy, but we don't need it since we
        # only mutate the `version_set` key.
        feature_dict["rpms"] = [r.copy() for r in aliased_rpms]

    # This is only None when there are no version sets in use.
    if version_set != None:
        vs_path = VERSION_SET_TO_PATH[version_set]

        # Resolve RPM names to version set targets.  We cannot avoid this
        # coupling with `rpm.bzl` because the same `image.rpms_install` may
        # be normalized against multiple version set names.
        for rpm_item in feature_dict.get("rpms", []):
            vs_name = rpm_item.get("version_set")
            if vs_name:
                rpm_item["version_set"] = tag_target(
                    target_tagger,
                    vs_path + "/rpm:" + vs_name,
                )
    else:
        for rpm_item in feature_dict.get("rpms", []):
            rpm_item.pop("version_set", None)

    direct_deps = []
    direct_deps.extend(feature.deps)
    direct_deps.extend(target_tagger.targets.keys())
    return feature_dict, direct_deps

def normalize_features(
        porcelain_targets_or_structs,
        human_readable_target,
        version_set):
    porcelain_targets_or_structs = _flatten_nested_lists(
        porcelain_targets_or_structs,
    )
    targets = []
    inline_features = []
    direct_deps = []
    for f in porcelain_targets_or_structs:
        if types.is_string(f):
            targets.append(
                PRIVATE_DO_NOT_USE_feature_target_name(f, version_set),
            )
        else:
            feature_dict, more_deps = _normalize_feature_and_get_deps(
                feature = f,
                version_set = version_set,
            )
            direct_deps.extend(more_deps)
            feature_dict["target"] = human_readable_target
            inline_features.append(feature_dict)

    return struct(
        targets = targets,
        inline_features = inline_features,
        direct_deps = direct_deps,
    )

def private_do_not_use_feature_json_genrule(
        name,
        deps,
        output_feature_cmd,
        visibility,
        version_set):
    name = PRIVATE_DO_NOT_USE_feature_target_name(name, version_set)
    buck_genrule(
        name = name,
        out = "feature.json",
        type = "image_feature",  # For queries
        # Future: It'd be nice to refactor `image_utils.bzl` and to use the
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
        fs_image_internal_rule = True,
    )

def image_feature(
        name,
        features,
        visibility = None,
        _test_only_version_sets = VERSION_SET_TO_PATH):
    """
    Turns a group of image actions into a Buck target, so it can be
    referenced from outside the current project via `//path/to:name`.

    Do NOT use this for composition within one project, just use a list.

    See the file docblock for more details on image action composition.

    See other `.bzl` files in this directory for actions that actually build
    the container (install RPMs, remove files/directories, create symlinks
    or directories, copy executable or data files, declare mounts).
    """
    for version_set in _test_only_version_sets:
        _image_feature_impl(
            name = name,
            features = features,
            visibility = get_visibility(visibility, name),
            version_set = version_set,
        )

def _image_feature_impl(name, features, visibility, version_set):
    # (1) Normalizes & annotates Buck target names so that they can be
    #     automatically enumerated from our JSON output.
    # (2) Builds a list of targets so that this converter can tell Buck
    #     that the `image_feature` depends on it.
    target_tagger = new_target_tagger()

    # Omit the ugly suffix here since this is meant only for humans to read while debugging.
    # For inline targets, `image_layer.bzl` sets this to the layer target path.
    human_readable_target = normalize_target(":" + name)
    normalized_features = normalize_features(
        features,
        human_readable_target,
        version_set,
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
        extra_deps = normalized_features.direct_deps + [
            # The `fake_macro_library` docblock explains this self-dependency
            "//fs_image/bzl/image_actions:feature",
        ],
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
            out = shell.quote(feature.items.to_json()),
        ),
        visibility = visibility,
        version_set = version_set,
    )
