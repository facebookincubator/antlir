# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/buck2/bzl/feature:feature.bzl", "FeatureInfo", "feature")
load("//antlir/buck2/bzl/feature:parent_layer.bzl", parent_layer_feature = "parent_layer")
load("//antlir/bzl:constants.bzl", "BZL_CONST", "REPO_CFG")
load("//antlir/bzl:flatten.bzl", "flatten")
load("//antlir/bzl:flavor_helpers.bzl", "flavor_helpers")
load("//antlir/bzl:query.bzl", "query")
load("//antlir/bzl:shape.bzl", "shape")
load(":build_appliance.bzl", "BuildApplianceInfo")
load(":ensure_single_output.bzl", "ensure_single_output")
load(":flavor.bzl", "FlavorInfo", "coerce_to_flavor_label")
load(":layer_info.bzl", "LayerInfo")
load(":layer_runnable_subtargets.bzl", "layer_runnable_subtargets", "layer_runtime_attr", "make_alias_with_equals_suffix")
load(":toolchain.bzl", "AntlirToolchainInfo")

def _impl(ctx: "context") -> ["provider"]:
    # Providing a flavor in combination with parent_layer is not necessary, but
    # can be used to add guarantees that the parent isn't swapped out to a new
    # flavor without forcing this child to acknowledge the change in cases where
    # that might be desirable.
    parent_flavor_label = None
    flavor = None
    if ctx.attrs.parent_layer:
        # see build_appliance.bzl for why this special case is necessary
        if BuildApplianceInfo in ctx.attrs.parent_layer:
            parent_flavor_label = ctx.attrs.parent_layer[BuildApplianceInfo].flavor_label
        else:
            flavor = ctx.attrs.parent_layer[LayerInfo].flavor
            parent_flavor_label = ctx.attrs.parent_layer[LayerInfo].flavor.label
    if parent_flavor_label and ctx.attrs.flavor:
        if parent_flavor_label != ctx.attrs.flavor.label:
            fail("parent_layer flavor is {}, but this layer is trying to use {}".format(
                parent_flavor_label,
                ctx.attrs.flavor.label,
            ))
        flavor_label = parent_flavor_label
    if not ctx.attrs.flavor and not ctx.attrs.parent_layer:
        fail("flavor is required with no parent_layer")
    if ctx.attrs.flavor and not ctx.attrs.parent_layer:
        flavor = ctx.attrs.flavor
        flavor_label = ctx.attrs.flavor.label
    if not ctx.attrs.flavor:
        flavor_label = parent_flavor_label
    else:
        flavor_label = ctx.attrs.flavor.label

    # TODO(T139523690) this should be part of the provider, not using this
    flavor_config = flavor_helpers.get_flavor_config(
        flavor_label.name,
        # TODO: buck2 layer does not support flavor_config_override yet
        flavor_config_override = None,
        assume_flavor_exists = True,
    )

    toolchain = ctx.attrs.toolchain[AntlirToolchainInfo]

    layer_out_dir = ctx.actions.declare_output("layer_out")

    targets_and_outputs = {
        label: output
        for label, output in {
            dep.label: ensure_single_output(dep, optional = True)
            for dep in ctx.attrs.buck1_features_deps
        }.items()
        if output
    }

    if ctx.attrs.parent_layer:
        targets_and_outputs[ctx.attrs.parent_layer.label] = ensure_single_output(ctx.attrs.parent_layer)

    # make sure to add the build appliance to the deps passed to the compiler
    # this does not need to handle the "build appliance is parent, so I don't
    # have a flavor" special case, since then that layer will already be
    # available as the parent_layer
    if flavor and flavor[FlavorInfo].build_appliance:
        build_appliance = flavor[FlavorInfo].build_appliance
        targets_and_outputs[build_appliance.label] = ensure_single_output(build_appliance)

    # antlir does not handle configured labels in all cases, so also record the
    # configuration-less labels
    configured_targets_and_outputs = dict(targets_and_outputs)
    for label, output in configured_targets_and_outputs.items():
        targets_and_outputs[label.raw_target()] = output

    # The discrepancy here between the `buck1/` dir and the `buck_version=2` is
    # because targets-and-outputs is a construct required by buck1. On buck2, we
    # can propagate dependencies without this mechanism, but we currently still
    # use it since Antlir is hardcoded to look in this map for certain
    # dependencies like the `parent_layer`. The `buck_version=2` informs the
    # targets_and_outputs library how to handle buck cell qualification, since
    # the target labels are always qualified when using buck2 and are rarely
    # qualified with a cell on buck1
    buck1_targets_and_outputs_json = ctx.actions.write_json("buck1/targets-and-outputs.json", {
        "metadata": {"buck_version": 2, "default_cell": ctx.label.cell},
        "targets_and_outputs": targets_and_outputs,
    })

    inner_build_cmd = cmd_args(
        [
            "#!/bin/bash -e",
            cmd_args('TMP="$TMPDIR"'),
            # The "version" code here ensures that the wrapper directory
            # has a unique name.  We could use `mktemp`, but our variant
            # is a little more predictable (not a security concern since
            # we own the parent directory) and a lot more debuggable.
            # Usability is better since our version sorts by build time.
            cmd_args(toolchain.subvolume_version, format = 'subvolume_ver="$({})"'),
            cmd_args(ctx.label.name, format = 'subvolume_wrapper_dir="{}:$subvolume_ver"'),
            # Do not touch $OUT until the very end so that if we
            # accidentally exit early with code 0, the rule still fails.
            cmd_args("mkdir -p $TMP/out"),
            # TODO: buck2 layer does not yet support mount_config (which is
            # almost completely unused anyway)
            cmd_args(toolchain.layer_mount_config, format = "{{}} {} > $TMP/out/mountconfig.json".format(shell.quote(str(ctx.label)))),
            cmd_args('layer_json="$TMP/out/layer.json"'),
            # IMPORTANT: This invalidates and/or deletes any existing
            # subvolume that was produced by the same target.  This is the
            # point of no return.
            #
            # This creates the wrapper directory for the subvolume, and
            # pre-initializes "$layer_json" in a special way to support a
            # form of refcounting that distinguishes between subvolumes that
            # are referenced from the Buck cache ("live"), and ones that are
            # no longer referenced ("dead").  We want to create the refcount
            # file before starting the build to guarantee that we have
            # refcount files for partially built images -- this makes
            # debugging failed builds a bit more predictable.
            cmd_args(
                toolchain.subvolume_garbage_collector,
                format = '{} --refcounts-dir=buck-out/.volume-refcount-hardlinks --subvolumes-dir="$SUBVOLUMES_DIR" --new-subvolume-wrapper-dir "$subvolume_wrapper_dir" --new-subvolume-json "$layer_json"',
            ),
            cmd_args(
                [
                    toolchain.compiler,
                    "--debug",
                    '--subvolumes-dir="$SUBVOLUMES_DIR"',
                    '--subvolume-rel-path="$subvolume_wrapper_dir"/volume',
                    cmd_args(toolchain.compiler, format = "--compiler-binary={}"),
                    cmd_args(ctx.attrs.buck1_features_json, format = "--child-feature-json={}"),
                    "--child-layer-target={}".format(shell.quote(str(ctx.label))),
                    "--flavor-config={}".format(shell.quote(shape.do_not_cache_me_json(flavor_config))),
                    cmd_args(buck1_targets_and_outputs_json, format = "--targets-and-outputs={}"),
                    cmd_args(ensure_single_output(ctx.attrs.parent_layer), format = "--parent-layer={}") if ctx.attrs.parent_layer else "",
                    cmd_args(REPO_CFG.host_mounts_allowed_in_targets, prepend = "--allowed-host-mount-target"),
                    '> "$layer_json"',
                ],
                delimiter = " \\\n  ",
            ),
            # Finally, produce the output at the right location
            'mv "$TMP/out" "$OUT"',
            "\n",
        ],
        delimiter = "\n",
    )
    inner_build_script = ctx.actions.write(
        "inner-build.sh",
        inner_build_cmd,
        is_executable = True,
    )
    build_cmd = cmd_args(
        [
            toolchain.builder,
            "--buck-version=2",
            "--label",
            shell.quote(str(ctx.label)),
            "--ensure-artifacts-dir-exists",
            toolchain.artifacts_dir,
            "--volume-for-repo",
            toolchain.volume_for_repo,
            '--tmp-dir="$TMPDIR"',
            cmd_args(layer_out_dir.as_output(), format = '--out="{}"'),
            "generic",
            "--",
            inner_build_script,
        ],
        delimiter = " \\\n  ",
    )
    build_script = ctx.actions.write(
        "build.sh",
        build_cmd,
        is_executable = True,
    )

    features_deps = list(ctx.attrs.features[FeatureInfo].deps.traverse())
    all_features_deps = features_deps + targets_and_outputs.values()

    ctx.actions.run(
        cmd_args(
            ["/bin/bash", "-e", build_script],
        ).hidden(
            inner_build_cmd,
            build_cmd,
            *all_features_deps
        ),
        category = "antlir_image_layer",
        local_only = True,
    )

    subvol_symlink = ctx.actions.declare_output("subvol_symlink")
    subvol_symlink_script = ctx.actions.write(
        "symlink.sh",
        cmd_args(
            cmd_args(
                'location="$(',
                toolchain.find_built_subvol,
                layer_out_dir,
                ')"',
                delimiter = " ",
            ),
            cmd_args(subvol_symlink.as_output(), format = 'ln -sf "$location" {}'),
            delimiter = "\n",
        ),
        is_executable = True,
    )
    ctx.actions.run(
        cmd_args(
            ["/bin/bash", "-e", subvol_symlink_script],
        ).hidden(
            toolchain.find_built_subvol,
            layer_out_dir,
            subvol_symlink.as_output(),
        ),
        category = "antlir_subvol_symlink",
        local_only = True,
    )

    sub_targets = {
        "build.sh": [DefaultInfo(
            default_outputs = [build_script],
        )],
        "subvol": [DefaultInfo(default_outputs = [subvol_symlink])],
    }
    sub_targets.update(
        layer_runnable_subtargets(
            ctx.attrs.nspawn_in_subvol_run,
            ctx.attrs.runtime,
            layer_out_dir,
        ),
    )

    return [
        LayerInfo(
            default_mountpoint = ctx.attrs.default_mountpoint,
            features = ctx.attrs.features,
            parent_layer = ctx.attrs.parent_layer,
            flavor = flavor,
            subvol_symlink = subvol_symlink,
        ),
        DefaultInfo(
            default_outputs = [layer_out_dir],
            sub_targets = sub_targets,
        ),
    ]

_layer = rule(
    impl = _impl,
    attrs = {
        # TODO(T139523690) when fully on buck2, the input to the compiler will
        # just have relative paths for everything, and deps will actually be
        # correct
        "buck1_features_deps": attrs.query(),
        # TODO(T139523690) just use 'features' when we kill the legacy buck1
        # inputs the compiler expects
        "buck1_features_json": attrs.source(),
        "default_mountpoint": attrs.option(attrs.string(), doc = "default mountpoint when used as the source of a layer mount", default = None),
        "features": attrs.dep(providers = [FeatureInfo]),
        "flavor": attrs.option(attrs.dep(providers = [FlavorInfo]), default = None),
        "nspawn_in_subvol_run": attrs.default_only(attrs.exec_dep(default = "//antlir/nspawn_in_subvol:run")),
        "parent_layer": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "runtime": layer_runtime_attr,
        "toolchain": attrs.default_only(attrs.toolchain_dep(default = "//antlir/buck2:toolchain")),
    },
)

def layer(
        *,
        name: str.type,
        flavor: str.type,
        # Features does not have a direct type hint, but it is still validated
        # by a type hint inside feature.bzl. Feature targets or
        # InlineFeatureInfo providers are accepted, at any level of nesting
        features = [],
        runtime: [str.type] = ["container"],
        **kwargs):
    parent_layer = kwargs.get("parent_layer")
    if parent_layer:
        features = [parent_layer_feature(layer = parent_layer)] + features

    features = flatten.flatten(features, item_type = ["InlineFeatureInfo", str.type])

    flavor = coerce_to_flavor_label(flavor)

    feature_target = name + "--features"
    feature(
        name = feature_target,
        visibility = [":" + name],
        features = features,
        flavors = [flavor],
    )
    feature_target = ":" + feature_target

    # TODO(T139523690)
    native.alias(
        name = name + BZL_CONST.layer_feature_suffix + BZL_CONST.PRIVATE_feature_suffix,
        actual = feature_target,
        visibility = kwargs.get("visibility"),
    )

    # TODO(T139523690) when fully on buck2, everything will be a provider and we
    # won't need to query shit like this
    deps_query = query.deps(flavor, 1) if flavor else query.set([])

    # our previously documented/well-known interface is targets like
    # `name=container`, so lets make some aliases until buck1 is dead and we can
    # support only the `name[subtarget]` form
    make_alias_with_equals_suffix(name, runtime)

    return _layer(
        name = name,
        features = feature_target,
        buck1_features_json = feature_target + "[buck1/features.json]",
        buck1_features_deps = deps_query,
        flavor = flavor,
        runtime = runtime,
        **kwargs
    )
