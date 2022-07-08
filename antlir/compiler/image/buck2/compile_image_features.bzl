# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Implementation detail for `image/layer/layer.bzl`, see its docs.
load("@bazel_skylib//lib:shell.bzl", "shell")
load(
    "//antlir/bzl:constants.bzl",
    "BZL_CONST",
    "REPO_CFG",
    "version_set_override_name",
)
load("//antlir/bzl:flavor_helpers.bzl", "flavor_helpers")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:query.bzl", "layer_deps_query", "query")
load("//antlir/bzl:shape.bzl", "shape")
load(
    "//antlir/bzl:target_helpers.bzl",
    "antlir_dep",
    "targets_and_outputs_arg_list",
)
load(
    "//antlir/compiler/image/feature/buck2:helpers.bzl",
    "is_build_appliance",
    "mark_path",
)
load(
    "//antlir/compiler/image/feature/buck2:new.bzl",
    feature_new_buck2 = "feature_new",
)
load(
    "//antlir/compiler/image/feature/buck2:rules.bzl",
    "maybe_add_feature_rule",
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

    flavor_config = flavor_helpers.get_flavor_config(flavor, flavor_config_override) if flavor else None

    if flavor_config:
        features.append(flavor_config.build_appliance)
    if parent_layer:
        features.append(maybe_add_feature_rule(
            name = "parent_layer",
            include_in_target_name = {"parent_layer": parent_layer},
            feature_shape = shape.new(
                shape.shape(
                    subvol = shape.field(shape.dict(str, str)),
                ),
                subvol = mark_path(parent_layer, is_layer = True),
            ),
            deps = [parent_layer],
        ))

    # This is the list of supported flavors for the features of the layer.
    # A value of `None` specifies that no flavor field was provided for the layer.
    flavors = [flavor] if flavor else None

    if not flavors and is_build_appliance(parent_layer):
        flavors = [flavor_helpers.get_flavor_from_build_appliance(parent_layer)]

    # Outputs the feature JSON for the given layer to disk so that it can be
    # parsed by other tooling.
    #
    # Keep in sync with `bzl_const.py`.
    features_for_layer = name + BZL_CONST.layer_feature_suffix
    feature_new_buck2(
        name = features_for_layer,
        features = features,
        flavors = flavors,
        parent_layer = parent_layer,
        visibility = ["//antlir/..."],
    )

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

    deps_query = query.union(
        [
            # We will query the deps of the features that are targets.
            query.deps(
                expr = query.attrfilter(
                    label = "type",
                    value = "image_feature",
                    expr = query.deps(
                        expr = query.set(features + [":" + features_for_layer]),
                        depth = query.UNBOUNDED,
                    ),
                ),
                depth = 1,
            ),
        ] + ([
            layer_deps_query(parent_layer),
        ] if parent_layer else []),
    )

    return '''
        # Take note of `targets_and_outputs` below -- this enables the
        # compiler to map the target sigils in the outputs
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
        quoted_child_feature_json_args = (
            "--child-feature-json $(location {})".format(
                ":" + features_for_layer,
            )
        ),
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
    )
