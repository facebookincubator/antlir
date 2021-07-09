# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Implementation detail for `image/layer/layer.bzl`, see its docs.
load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:sha256.bzl", "sha256_b64")
load("//antlir/bzl/image/feature:new.bzl", "FEATURES_FOR_LAYER_PREFIX", "feature_new", "normalize_features")
load(":constants.bzl", "REPO_CFG")
load(":query.bzl", "layer_deps_query", "query")
load(":target_helpers.bzl", "targets_and_outputs_arg_list")
load(":target_tagger.bzl", "new_target_tagger", "tag_target", "target_tagger_to_feature")

def compile_image_features(
        name,
        current_target,
        parent_layer,
        features,
        flavor_config,
        subvol_name = None):
    '''
    Arguments

    - `subvol_name`: Future: eliminate this argument so that the build-time
    hardcodes this to "volume". Move this setting into btrfs-specific
    `image.package` options. See this post for more details
    https://fburl.com/diff/3050aw26
    '''
    if features == None:
        features = []

    target_tagger = new_target_tagger()

    if flavor_config.build_appliance:
        features.append(target_tagger_to_feature(
            target_tagger,
            struct(),
            extra_deps = [flavor_config.build_appliance],
        ))

    # Outputs the feature JSON for the given layer to disk so that it can be
    # parsed by other tooling.
    features_for_layer = FEATURES_FOR_LAYER_PREFIX + name
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
        flavors = [flavor_config.name],
    )
    normalized_features = normalize_features(
        [":" + features_for_layer],
        current_target,
        flavor = flavor_config.name,
    )

    vset_override_name = None
    if flavor_config.rpm_version_set_overrides:
        vset_override_name = "vset-override-" + sha256_b64(current_target)
        buck_genrule(
            name = vset_override_name,
            out = "unused",
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

    return '''
        # Take note of `targets_and_outputs` below -- this enables the
        # compiler to map the `target_tagger` target sigils in the outputs
        # of `feature`s to those targets' outputs.
        #
        # `exe` vs `location` is explained in `image_package.py`.
        #
        # We access `ANTLIR_DEBUG` because this is never expected to
        # change the output, so it's deliberately not a Buck input.
        $(exe //antlir:compiler) {maybe_artifacts_require_repo} \
          ${{ANTLIR_DEBUG:+--debug}} \
          --subvolumes-dir "$subvolumes_dir" \
          --subvolume-rel-path \
            "$subvolume_wrapper_dir/"{subvol_name_quoted} \
          --flavor {flavor_quoted} \
          {maybe_unsafe_bypass_flavor_check} \
          {maybe_quoted_build_appliance_args} \
          {maybe_quoted_rpm_installer_args} \
          {maybe_quoted_rpm_repo_snapshot_args} \
          {maybe_allowed_host_mount_target_args} \
          {maybe_version_set_override} \
          --child-layer-target {current_target_quoted} \
          {quoted_child_feature_json_args} \
          {targets_and_outputs} \
              > "$layer_json"

        # Insert the outputs of the queried dependencies to short-circuit
        # the dep-graph. This will ensure that this target gets rebuilt
        # if any dep returned by the query has changed. This is a bit of
        # an unfortunate requirement due to the non-cachable nature of this
        # rule.
        # $(query_outputs '{deps_query}')
    '''.format(
        subvol_name_quoted = shell.quote(subvol_name or "volume"),
        current_target_quoted = shell.quote(current_target),
        flavor_quoted = shell.quote(flavor_config.name),
        maybe_unsafe_bypass_flavor_check = (
            "--unsafe-bypass-flavor-check" if flavor_config.unsafe_bypass_flavor_check else ""
        ),
        quoted_child_feature_json_args = " ".join([
            "--child-feature-json $(location {})".format(t)
            for t in normalized_features.targets
        ] + (
            ["--child-feature-json <(echo {})".format(shell.quote(struct(
                features = normalized_features.inline_features,
                target = current_target,
            ).to_json()))] if normalized_features.inline_features else []
        )),
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
        maybe_quoted_build_appliance_args = (
            "--build-appliance-buck-out $(location {})".format(
                flavor_config.build_appliance,
            ) if flavor_config.build_appliance else ""
        ),
        maybe_quoted_rpm_installer_args = (
            "--rpm-installer {}".format(
                shell.quote(flavor_config.rpm_installer),
            ) if flavor_config.rpm_installer else ""
        ),
        maybe_quoted_rpm_repo_snapshot_args = (
            "--rpm-repo-snapshot {}".format(
                shell.quote(flavor_config.rpm_repo_snapshot),
            ) if flavor_config.rpm_repo_snapshot else ""
        ),
        maybe_version_set_override = (
            "--version-set-override $(location :{})".format(vset_override_name) if vset_override_name else ""
        ),
    )
