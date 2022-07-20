# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Implementation detail for `image/layer/layer.bzl`, see its docs.
load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl/image/feature:new.bzl", "feature_new", "normalize_features")
load(":constants.bzl", "BZL_CONST", "REPO_CFG", "version_set_override_name")
load(":flavor_helpers.bzl", "flavor_helpers")
load(":query.bzl", "layer_deps_query", "query")
load(":target_helpers.bzl", "antlir_dep", "targets_and_outputs_arg_list")
load(":target_tagger.bzl", "new_target_tagger", "tag_target", "target_tagger_to_feature")

def check_flavor(
        flavor,
        parent_layer,
        flavor_config_override,
        name,
        current_target):
    if not flavor:
        if parent_layer and flavor_config_override:
            # We throw this error because the default flavor can differ
            # from the flavor set in the parent layer making the override
            # invalid.
            fail(
                "If you set `flavor_config_override` together with `parent_layer`, " +
                "you must explicitly set `flavor` to  the parent's `flavor`.",
            )
        elif not parent_layer:
            fail("Build for {}, target {} failed: either `flavor` or `parent_layer` must be provided.".format(name, current_target))

def vset_override_genrule(flavor_config, current_target):
    vset_override_name = None
    if flavor_config and flavor_config.rpm_version_set_overrides:
        vset_override_name = version_set_override_name(current_target)
        buck_genrule(
            name = vset_override_name,
            bash = """
cat > "$OUT" << 'EOF'
{envra_file_contents}
EOF
            """.format(
                envra_file_contents = "\n".join(["\t".join([
                    nevra.epoch,
                    nevra.name,
                    nevra.version,
                    nevra.release,
                    nevra.arch,
                ]) for nevra in flavor_config.rpm_version_set_overrides]),
            ),
            antlir_rule = "user-internal",
        )
    return vset_override_name

def compile_image_features_output(
        name,
        current_target,
        parent_layer,
        flavor,
        flavor_config,
        subvol_name,
        internal_only_is_genrule_layer,
        vset_override_name,
        deps_query,
        quoted_child_feature_json_args):
    maybe_profile = ""
    profile_dir = native.read_config("antlir", "profile", None)
    if profile_dir:
        maybe_profile = "--profile={}".format(profile_dir)

    return '''
        # Take note of `targets_and_outputs` below -- this enables the
        # compiler to map the `target_tagger` target sigils in the outputs
        # of `feature`s to those targets' outputs.
        #
        # `exe` vs `location` is explained in `image_package.py`.
        #
        # We access `ANTLIR_DEBUG` because this is never expected to
        # change the output, so it's deliberately not a Buck input.
        $(exe {compiler}) {maybe_artifacts_require_repo} \
          ${{ANTLIR_DEBUG:+--debug}} \
          --subvolumes-dir "$subvolumes_dir" \
          --subvolume-rel-path \
            "$subvolume_wrapper_dir/"{subvol_name_quoted} \
          {maybe_flavor_config} \
          {maybe_allowed_host_mount_target_args} \
          {maybe_version_set_override} \
          {maybe_parent_layer} \
          --child-layer-target {current_target_quoted} \
          {quoted_child_feature_json_args} \
          {targets_and_outputs} \
          --compiler-binary $(location {compiler}) \
          {internal_only_is_genrule_layer} \
          {maybe_profile} \
              > "$layer_json"

        # Insert the outputs of the queried dependencies to short-circuit
        # the dep-graph. This will ensure that this target gets rebuilt
        # if any dep returned by the query has changed. This is a bit of
        # an unfortunate requirement due to the non-cachable nature of this
        # rule.
        # $(query_outputs '{deps_query}')
    '''.format(
        compiler = antlir_dep(":compiler"),
        subvol_name_quoted = shell.quote(subvol_name or "volume"),
        current_target_quoted = shell.quote(current_target),
        quoted_child_feature_json_args = quoted_child_feature_json_args,
        maybe_flavor_config = (
            "--flavor-config {}".format(
                shell.quote(shape.do_not_cache_me_json(flavor_config)),
            ) if flavor_config else ""
        ),
        maybe_allowed_host_mount_target_args = (
            " ".join([
                "--allowed-host-mount-target={}".format(t.strip())
                for t in REPO_CFG.host_mounts_allowed_in_targets
            ])
        ),
        # We will ask Buck to ensure that the outputs of the direct
        # dependencies of our `feature`s are available on local disk.
        #
        # See `Implementation notes: Dependency resolution` in `__doc__`.
        # Note that we need no special logic to exclude parent-layer
        # features -- this query does not traverse them anyhow, since the
        # the parent layer feature is added as an "inline feature" above.
        targets_and_outputs = " ".join(targets_and_outputs_arg_list(
            name = name,
            query = deps_query,
        )),
        deps_query = deps_query,
        maybe_artifacts_require_repo = (
            "--artifacts-may-require-repo" if
            # Future: Consider **only** emitting this flag if the image is
            # actually contains executables (via `install_buck_runnable`).
            # NB: This may not actually be 100% doable at macro parse time,
            # since `install_buck_runnable_tree` does not know if it is
            # installing an executable file or a data file until build-time.
            # That said, the parse-time test would already narrow the scope
            # when the repo is mounted, and one could potentially extend the
            # compiler to further modulate this flag upon checking whether
            # any executables were in fact installed.
            REPO_CFG.artifacts_require_repo else ""
        ),
        maybe_version_set_override = (
            "--version-set-override $(location :{})".format(vset_override_name) if vset_override_name else ""
        ),
        maybe_parent_layer = (
            "--parent-layer $(location {})".format(parent_layer) if parent_layer and not flavor else ""
        ),
        internal_only_is_genrule_layer = "--internal-only-is-genrule-layer" if internal_only_is_genrule_layer else "",
        maybe_profile = maybe_profile,
    )

def compile_image_features(
        name,
        current_target,
        parent_layer,
        features,
        flavor,
        flavor_config_override,
        subvol_name = None,
        internal_only_is_genrule_layer = False):
    '''
    Arguments

    - `subvol_name`: Future: eliminate this argument so that the build-time
    hardcodes this to "volume". Move this setting into btrfs-specific
    `package.new` options. See this post for more details
    https://fburl.com/diff/3050aw26
    '''
    if features == None:
        features = []

    target_tagger = new_target_tagger()

    check_flavor(
        flavor,
        parent_layer,
        flavor_config_override,
        name,
        current_target,
    )

    flavor_config = flavor_helpers.get_flavor_config(flavor, flavor_config_override) if flavor else None

    if flavor_config and flavor_config.build_appliance:
        features.append(target_tagger_to_feature(
            target_tagger,
            struct(),
            extra_deps = [flavor_config.build_appliance],
        ))

    # This is the list of supported flavors for the features of the layer.
    # A value of `None` specifies that no flavor field was provided for the layer.
    flavors = [flavor] if flavor else None

    # Outputs the feature JSON for the given layer to disk so that it can be
    # parsed by other tooling.
    #
    # Keep in sync with `bzl_const.py`.
    features_for_layer = name + BZL_CONST.layer_feature_suffix
    feature_new(
        name = features_for_layer,
        features = features + (
            [target_tagger_to_feature(
                target_tagger,
                items = struct(parent_layer = [{"subvol": tag_target(
                    target_tagger,
                    parent_layer,
                    is_layer = True,
                )}]),
            )] if parent_layer else []
        ),
        flavors = flavors,
    )
    normalized_features = normalize_features(
        [":" + features_for_layer],
        current_target,
        flavors = flavors,
    )

    vset_override_name = vset_override_genrule(flavor_config, current_target)

    deps_query = query.union(
        [
            # For inline `feature`s, we already know the direct deps.
            query.set(normalized_features.direct_deps),
            # We will query the deps of the features that are targets.
            query.deps(
                expr = query.attrfilter(
                    label = "type",
                    value = "image_feature",
                    expr = query.deps(
                        expr = query.set(normalized_features.targets),
                        depth = query.UNBOUNDED,
                    ),
                ),
                depth = 1,
            ),
        ] + ([
            layer_deps_query(parent_layer),
        ] if parent_layer else []),
    )

    quoted_child_feature_json_args = " ".join([
        "--child-feature-json $(location {})".format(t)
        for t in normalized_features.targets
    ] + (
        ["--child-feature-json <(echo {})".format(shell.quote(
            structs.as_json(struct(
                features = normalized_features.inline_features,
                target = current_target,
            )),
        ))] if normalized_features.inline_features else []
    ))

    return compile_image_features_output(
        name,
        current_target,
        parent_layer,
        flavor,
        flavor_config,
        subvol_name,
        internal_only_is_genrule_layer,
        vset_override_name,
        deps_query,
        quoted_child_feature_json_args,
    )
