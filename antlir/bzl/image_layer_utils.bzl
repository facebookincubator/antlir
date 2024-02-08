# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("@bazel_skylib//lib:types.bzl", "types")
load(":bash.bzl", "wrap_bash_build_in_common_boilerplate")
load(":build_defs.bzl", "buck_genrule", "config", "is_buck2")
load(":image_layer_runtime.bzl", "add_runtime_targets")
load(":structs.bzl", "structs")
load(":target_helpers.bzl", "antlir_dep", "normalize_target")

def _image_layer_impl(
        _rule_type,
        _layer_name,
        _make_subvol_cmd,
        _deps_query,
        # Layers can be used in the `mounts` field of an `feature`.
        # This setting affects how **this** layer may be mounted inside
        # others.
        #
        # This argument may be a dict, or a target path whose outputs is a
        # JSON dict of the same form.  The latter was added to allow
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
        # For each element set within runtime, an additional target labelled with the suffix `=<runtime>` will be emitted.
        # A target with runtime suffix `container` is always emitted by default.
        # See [docs](/docs/tutorials/helper-buck-targets#imagelayer).
        runtime = None,
        labels = None,
        visibility = None):
    visibility = visibility or []
    if mount_config == None:
        mount_config = {}

    # The buck-out dir can contain multiple dirs, we only need the top level one
    buck_out_base_dir = config.get_buck_out_path().split("/")[0]

    # IMPORTANT: If you touch this genrule, update `image_layer_alias`.
    buck_genrule(
        name = _layer_name,
        bash = wrap_bash_build_in_common_boilerplate(
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
                --subvolumes-dir "$SUBVOLUMES_DIR" \
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
                    normalize_target(":" + _layer_name),
                ),
                make_subvol_cmd = _make_subvol_cmd,
                # To make layers "image-mountable", provide `mountconfig.json`.
                print_mount_config = (
                    # `mount_config` was a target path
                    "cat $(location {})".format(mount_config)
                ) if types.is_string(mount_config) else (
                    # inline `mount_config` dict
                    "echo {}".format(
                        shell.quote(structs.as_json(struct(**mount_config))),
                    )
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
                    buck_out = buck_out_base_dir,
                    parameter_expand = "{" + "GEN_DIR%%/{buck_out}/*".format(
                        buck_out = buck_out_base_dir,
                    ) + "}",
                ) if not is_buck2() else "buck-out/.volume-refcount-hardlinks",
                subvolume_garbage_collector = antlir_dep(":subvolume-garbage-collector"),
                subvolume_version = antlir_dep(":subvolume-version"),
            ),
            target_name = _layer_name,
            deps_query = _deps_query,
        ),
        # Layers are only usable on the same host that built them, so
        # keep our output JSON out of the distributed Buck cache.  See
        # the docs for BuildRule::isCacheable.
        cacheable = False,
        labels = ["image_layer", "uses_sudo"] + (labels or []),
        type = _rule_type,  # For queries
        visibility = visibility,
        # @oss-disable
    )

    add_runtime_targets(_layer_name, runtime)

image_layer_utils = struct(
    image_layer_impl = _image_layer_impl,
)
