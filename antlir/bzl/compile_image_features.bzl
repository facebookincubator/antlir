# Implementation detail for `image_layer.bzl`, see its docs.
load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:constants.bzl", "DO_NOT_USE_BUILD_APPLIANCE", "REPO_CFG")
load("//antlir/bzl/image_actions:feature.bzl", "normalize_features")
load(":rpm_repo_snapshot.bzl", "snapshot_install_dir")
load(":structs.bzl", "structs")
load(":target_tagger.bzl", "new_target_tagger", "tag_target", "target_tagger_to_feature")

def _build_opts(
        # The name of the btrfs subvolume to create.
        subvol_name = "volume",
        # Path to a layer target of a build appliance, containing an
        # installed `rpm_repo_snapshot()`, plus an OS image with other
        # image build tools like `btrfs`, `dnf`, `yum`, `tar`, `ln`, ...
        build_appliance = REPO_CFG.build_appliance_default,
        # A "version set" name, see `bzl/constants.bzl`.
        # Currently used for RPM version locking.
        #
        # Future: refer to the OSS "version selection" doc once ready.
        version_set = REPO_CFG.version_set_default,
        # The build appliance currently does not set a default package
        # manager -- in non-default settings, this has to be chosen per
        # image, since a BA can support multiple package managers.  In the
        # future, if specifying a non-default installer per image proves
        # onerous when using non-default BAs, we could support a `default`
        # symlink under `default-snapshot-for-installer`.
        rpm_installer = REPO_CFG.rpm_installer_default,
        # Syntactically, this is a Buck target path.  But, it is **not**
        # used to depend on a Buck target.  A target may not even exist in
        # the current repo at this path.  Rather, this target path is
        # normalized, mangled, and then used to select a non-default child
        # of `/__antlir__/rpm/repo-snapshot/` in the build appliance.  So
        # this refers to a target that got incorporated into the build
        # appliance, whenever that image was built.  `None` uses the default
        # determined by looking up `rpm_installer` in
        # `/__antlir__/rpm/repo-snapshot/default-snapshot-for-installer`.
        rpm_repo_snapshot = None,
        # By default `RpmActionItem` will not populate
        # `/var/cache/{dnf,yum}` in the built image.  We set this flag to
        # `True` for the special case of a build appliance (BA) image.  It
        # is beneficial to have the BA's cache populated because it speeds
        # up `RpmActionItem` in builds based on this BA.
        preserve_yum_dnf_cache = False):
    if build_appliance == None:
        fail(
            "Must be a target path, or a value from `constants.bzl`",
            "build_appliance",
        )

    # When building the BA itself, we need this constant to avoid a circular
    # dependency.
    #
    # This feature is exposed a non-`None` magic constant so that callers
    # cannot get confused whether `None` refers to "no BA" or "default BA".
    if build_appliance == DO_NOT_USE_BUILD_APPLIANCE:
        build_appliance = None

    if version_set not in REPO_CFG.version_set_to_path:
        fail(
            "Must be in {}".format(list(REPO_CFG.version_set_to_path)),
            "version_set",
        )
    return struct(
        build_appliance = build_appliance,
        preserve_yum_dnf_cache = preserve_yum_dnf_cache,
        version_set = version_set,
        rpm_installer = rpm_installer,
        rpm_repo_snapshot = (
            snapshot_install_dir(rpm_repo_snapshot) if rpm_repo_snapshot else None
        ),
        subvol_name = subvol_name,
    )

def _query_set(target_paths):
    'Returns `set("//foo:target1" "//bar:target2")` for use in Buck queries.'

    if not target_paths:
        return "set()"

    # This does not currently escape double-quotes since Buck docs say they
    # cannot occur: https://buck.build/concept/build_target.html
    return 'set("' + '" "'.join(target_paths) + '")'

def compile_image_features(
        current_target,
        parent_layer,
        features,
        build_opts):
    if features == None:
        features = []

    build_opts = _build_opts(**(structs.to_dict(build_opts) if build_opts else {}))
    target_tagger = new_target_tagger()
    normalized_features = normalize_features(
        features + (
            [target_tagger_to_feature(
                target_tagger,
                items = struct(parent_layer = [{"subvol": tag_target(
                    target_tagger,
                    parent_layer,
                    is_layer = True,
                )}]),
            )] if parent_layer else []
        ),
        current_target,
        version_set = build_opts.version_set,
    )

    return '''
        # Take note of `targets_and_outputs` below -- this enables the
        # compiler to map the `target_tagger` target sigils in the outputs
        # of `image_feature` to those targets' outputs.
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
          {maybe_quoted_build_appliance_args} \
          {maybe_quoted_rpm_installer_args} \
          {maybe_quoted_rpm_repo_snapshot_args} \
          {maybe_preserve_yum_dnf_cache_args} \
          {maybe_allowed_host_mount_target_args} \
          --child-layer-target {current_target_quoted} \
          {quoted_child_feature_json_args} \
          --child-dependencies {feature_deps_query_macro} \
              > "$layer_json"
    '''.format(
        subvol_name_quoted = shell.quote(build_opts.subvol_name),
        current_target_quoted = shell.quote(current_target),
        quoted_child_feature_json_args = " ".join([
            "--child-feature-json $(location {})".format(t)
            for t in normalized_features.targets
        ] + (
            ["--child-feature-json <(echo {})".format(shell.quote(struct(
                target = current_target,
                features = normalized_features.inline_features,
            ).to_json()))] if normalized_features.inline_features else []
        )),
        maybe_allowed_host_mount_target_args = (
            " ".join([
                "--allowed-host-mount-target={}".format(t.strip())
                for t in REPO_CFG.host_mounts_allowed_in_targets
            ])
        ),
        # We will ask Buck to ensure that the outputs of the direct
        # dependencies of our `image_feature`s are available on local disk.
        #
        # See `Implementation notes: Dependency resolution` in `__doc__`.
        # Note that we need no special logic to exclude parent-layer
        # features -- this query does not traverse them anyhow, since the
        # the parent layer feature is added as an "inline feature" above.
        #
        # We have two layers of quoting here.  The outer '' groups the query
        # into a single argument for `query_targets_and_outputs`.  Then,
        # `_query_set` double-quotes each target name to allow the use of
        # special characters like `=` in target names.
        feature_deps_query_macro = """$(query_targets_and_outputs '
            {direct_deps_set} union
            deps(attrfilter(type, image_feature, deps({feature_set})), 1)
        ')""".format(
            # For inline `image.feature`s, we already know the direct deps.
            direct_deps_set = _query_set(normalized_features.direct_deps),
            # We will query the direct deps of the features that are targets.
            feature_set = _query_set(normalized_features.targets),
        ),
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
            "--build-appliance-json $(location {})/layer.json".format(
                build_opts.build_appliance,
            ) if build_opts.build_appliance else ""
        ),
        maybe_quoted_rpm_installer_args = (
            "--rpm-installer {}".format(
                shell.quote(build_opts.rpm_installer),
            ) if build_opts.rpm_installer else ""
        ),
        maybe_quoted_rpm_repo_snapshot_args = (
            "--rpm-repo-snapshot {}".format(
                shell.quote(build_opts.rpm_repo_snapshot),
            ) if build_opts.rpm_repo_snapshot else ""
        ),
        maybe_preserve_yum_dnf_cache_args = (
            "--preserve-yum-dnf-cache" if build_opts.preserve_yum_dnf_cache else ""
        ),
    )
