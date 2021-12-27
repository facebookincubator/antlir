# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("@bazel_skylib//lib:types.bzl", "types")
load(":image_utils.bzl", "image_utils")
load(":oss_shim.bzl", "buck_command_alias", "buck_genrule", "config", "is_buck2")
load(":query.bzl", "layer_deps_query")
load(":target_helpers.bzl", "antlir_dep", "targets_and_outputs_arg_list")

def _add_run_in_subvol_target(name, kind, extra_args = None):
    target = name + "=" + kind
    buck_command_alias(
        name = target,
        exe = antlir_dep("nspawn_in_subvol:run"),
        args = [
            "--layer",
            "$(location {})".format(shell.quote(":" + name)),
        ] + (extra_args or []) + targets_and_outputs_arg_list(
            name = target,
            query = layer_deps_query(
                layer = image_utils.current_target(name),
            ),
        ),
        antlir_rule = "user-internal",
    )

# In an attempt to preserve some form of backwards compatibility,
# create targets that will be invoked when users use deprecated
# helper targets (eg. -boot) for a more pleasant and informative failure.
# These targets are intended to exist temporarily, and should be deleted
# once all deprecated helper targets are deemed archaic enough.
def _add_fail_with_message_target(name, kind, message):
    target = name + "-" + kind
    buck_command_alias(
        name = target,
        exe = antlir_dep("bzl:fail-with-message"),
        args = ["--message", message],
        antlir_rule = "user-internal",
    )

def container_target_name(name):
    return name + "=container"

def _image_layer_impl(
        _rule_type,
        _layer_name,
        _make_subvol_cmd,
        # For now, layer implementations mark this explicitly.  I doubt that
        # "antlir-private" is a sensible default here.
        antlir_rule,
        # Layers can be used in the `mounts` field of an `feature`.
        # This setting affects how **this** layer may be mounted inside
        # others.
        #
        # This argument may be a dict, or a target path whose outputs is a
        # JSON dict of the same form.  The latter as added to allow
        # generating mount configs for fetched packages.
        #
        # The default mount config for a layer only provides a
        # `build_source`, specifying how the layer should be mounted at
        # development time inside the in-repo `buck-image-out` subtree.
        #
        # This argument can set `runtime_source` and `default_mountpoint`.
        # The former is essential -- to get a layer from `mounts` to be
        # mounted at container run-time, we have to tell the container agent
        # how to obtain the layer-to-be-mounted.  This can be done in a
        # variety of ways, so it's not part of `image.layer` itself, and
        # must be set from outside.
        mount_config = None,
        # Most use-cases should never need to set this.  A string is used
        # instead of int because Starlark supports only 32-bit integers.
        # Future:
        #  (i) Should we determine this dynamically from the installed
        #      artifacts (by totaling up the bytes needed for copied files,
        #      RPMs, tarballs, etc)?  NB: At the moment, this number doesn't
        #      work precisely as a user would want -- we just require that
        #      the base volume have at least this much space, -- but
        #      hopefully people don't have to change it too much.
        # (ii) For sendstreams, it's much more plausible to correctly
        #      estimate the size requirements, so we might do that sooner.
        # The default is `LOOP_SIZE`, see definition in
        # `antlir/volume_for_repo.py`. Setting this here would introduce an
        # issue. See an explanation above definition.
        layer_size_bytes = None,
        # For each element set within runtime, an additional target labelled with the suffix `=<runtime>` will be emitted.
        # A target with runtime suffix `container` is always emitted by default.
        # See [docs](/docs/tutorials/helper-buck-targets#imagelayer).
        runtime = None,
        visibility = None):
    runtime = runtime or []
    if "container" not in runtime:
        runtime.append("container")
    visibility = visibility or []
    if mount_config == None:
        mount_config = {}

    # The buck-out dir can contain multiple dirs, we only need the top level one
    buck_out_base_dir = config.get_buck_out_path().split("/")[0]

    # IMPORTANT: If you touch this genrule, update `image_layer_alias`.
    buck_genrule(
        name = _layer_name,
        bash = image_utils.wrap_bash_build_in_common_boilerplate(
            self_dependency = antlir_dep("bzl:image_layer"),
            bash = '''
            # We want subvolume names to be user-controllable. To permit
            # this, we wrap each subvolume in a temporary subdirectory.
            # This also allows us to prevent access to capability-
            # escalating programs inside the built subvolume by users
            # other than the repo owner.
            #
            # The "version" code here ensures that the wrapper directory
            # has a unique name.  We could use `mktemp`, but our variant
            # is a little more predictable (not a security concern since
            # we own the parent directory) and a lot more debuggable.
            # Usability is better since our version sorts by build time.
            #
            # `exe` vs `location` is explained in `image_package.py`.
            # `exe` won't expand in \\$( ... ), so we need `binary_path`.
            binary_path=( $(exe {subvolume_version}) )
            subvolume_ver=\\$( "${{binary_path[@]}}" )
            subvolume_wrapper_dir={layer_name_mangled_quoted}":$subvolume_ver"

            # Do not touch $OUT until the very end so that if we
            # accidentally exit early with code 0, the rule still fails.
            mkdir -p "$TMP/out"
            {print_mount_config} |
                $(exe {layer_mount_config}) {layer_target_quoted} \
                    > "$TMP/out/mountconfig.json"
            # "layer.json" points at the subvolume inside `buck-image-out`.
            layer_json="$TMP/out/layer.json"

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
            refcounts_dir=\\$( readlink -f {refcounts_dir_quoted} )
            # `exe` vs `location` is explained in `image_package.py`
            $(exe {subvolume_garbage_collector}) \
                --refcounts-dir "$refcounts_dir" \
                --subvolumes-dir "$subvolumes_dir" \
                --new-subvolume-wrapper-dir "$subvolume_wrapper_dir" \
                --new-subvolume-json "$layer_json"

            {make_subvol_cmd}

            mv "$TMP/out" "$OUT"  # Allow the rule to succeed.
            '''.format(
                layer_mount_config = antlir_dep(":layer-mount-config"),
                # Buck target names permit `/`, but we want a 1-level
                # hierarchy for layer wrapper directories in
                # `buck-image-out`, so mangle `/`.
                layer_name_mangled_quoted = shell.quote(
                    _layer_name.replace("/", "=="),
                ),
                layer_target_quoted = shell.quote(
                    image_utils.current_target(_layer_name),
                ),
                # The `buck-out` path is configurable in buck and we should not
                # hard code it. Unfortunately there is no good way to discover
                # the full abs path of this configured dir from bzl. So we use a
                # bash parameter expansion to figure this out via the provided
                # GEN_DIR environment variable. The tricky thing is that you
                # can't have nested substitutions in starlark, and to use a bash
                # parameter expansion it must be wrapped with ${}. To work
                # around this we awkwardly construct the "inner" parameter
                # expansion, hug it with `{` and `}`, and then finally insert
                # it into the full expression to yield something that looks
                # like (assuming `buck-out` is the configured path):
                # ${GEN_DIR%%/buck-out/*}/buck-out/.volume-refcount-hardlinks/
                refcounts_dir_quoted = "${parameter_expand}/{buck_out}/.volume-refcount-hardlinks/".format(
                    parameter_expand = "{" + "GEN_DIR%%/{buck_out}/*".format(
                        buck_out = buck_out_base_dir,
                    ) + "}",
                    buck_out = buck_out_base_dir,
                ) if not is_buck2() else "buck-out/.volume-refcount-hardlinks",
                make_subvol_cmd = _make_subvol_cmd,
                # To make layers "image-mountable", provide `mountconfig.json`.
                print_mount_config = (
                    # `mount_config` was a target path
                    "cat $(location {})".format(mount_config)
                ) if types.is_string(mount_config) else (
                    # inline `mount_config` dict
                    "echo {}".format(
                        shell.quote(struct(**mount_config).to_json()),
                    )
                ),
                subvolume_garbage_collector = antlir_dep(":subvolume-garbage-collector"),
                subvolume_version = antlir_dep(":subvolume-version"),
            ),
            rule_type = _rule_type,
            target_name = _layer_name,
            volume_min_free_bytes = layer_size_bytes,
        ),
        # Layers are only usable on the same host that built them, so
        # keep our output JSON out of the distributed Buck cache.  See
        # the docs for BuildRule::isCacheable.
        cacheable = False,
        type = _rule_type,  # For queries
        visibility = visibility,
        antlir_rule = antlir_rule,
        labels = ["image_layer"],
    )

    for elem in runtime:
        if elem == "container":
            _add_run_in_subvol_target(_layer_name, "container")
            _add_fail_with_message_target(
                _layer_name,
                "container",
                "The '-container' helper target is deprecated, use '=container' instead.",
            )
        elif elem == "systemd":
            _add_run_in_subvol_target(_layer_name, "systemd", extra_args = ["--boot", "--append-console"])
            _add_fail_with_message_target(
                _layer_name,
                "boot",
                "The '-boot' helper target is deprecated, use '=systemd' instead.",
            )
        else:
            fail("Unsupported runtime encountered: {}".format(elem))

image_layer_utils = struct(
    image_layer_impl = _image_layer_impl,
)
