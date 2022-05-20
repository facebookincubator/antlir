# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:bash.bzl", "wrap_bash_build_in_common_boilerplate")
load("//antlir/bzl:loopback_opts.bzl", "normalize_loopback_opts")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load(":btrfs.shape.bzl", "btrfs_opts_t", "btrfs_subvol_t")

def _new_btrfs_subvol(**kwargs):
    return shape.new(
        btrfs_subvol_t,
        **kwargs
    )

_btrfs_subvol_api = struct(
    new = _new_btrfs_subvol,
    t = btrfs_subvol_t,
)

def _new_btrfs_opts(subvols, default_subvol = None, loopback_opts = None, **kwargs):
    if default_subvol and not default_subvol.startswith("/"):
        fail("Default subvol must be an absolute path: " + default_subvol)

    return shape.new(
        btrfs_opts_t,
        subvols = subvols,
        default_subvol = default_subvol,
        loopback_opts = normalize_loopback_opts(loopback_opts),
        **kwargs
    )

_btrfs_opts_api = struct(
    new = _new_btrfs_opts,
    subvol = _btrfs_subvol_api,
    t = btrfs_opts_t,
)

def _new_btrfs(
        name,
        # Opts are required
        opts,
        # Buck `labels` to add to the resulting target; aka `tags` in fbcode.
        labels = None,
        visibility = None,
        antlir_rule = "user-facing"):
    visibility = visibility or []

    if not opts:
        fail("`opts` is required for btrfs.new")

    # For queries
    _rule_type = "image-package-btrfs"

    # All the layers being built
    layers = []
    for subvol_name, subvol in opts.subvols.items():
        if not subvol_name.startswith("/"):
            fail("Requested subvol names must be absolute paths: " + subvol_name)

        layers.append(subvol.layer)

    opts_name = name + "__opts"
    buck_genrule(
        name = opts_name,
        out = "opts.json",
        cmd = "echo {} > $OUT".format(shell.quote(shape.do_not_cache_me_json(opts))),
        cacheable = False,
        antlir_rule = antlir_rule,
    )

    buck_genrule(
        name = name,
        out = "image.btrfs",
        type = _rule_type,
        bash = wrap_bash_build_in_common_boilerplate(
            self_dependency = antlir_dep("bzl/image/package:btrfs"),
            bash = '''
            # Create the file as the build user first
            touch "$OUT"
            # Packaging currently requires root but to avoid
            # sprinkling sudo calls through out we just run the
            # entire packaging engine as root.  This makes it
            # less fragile for future improvements when we can
            # run this in a user namespace or container to avoid
            # root execution on the build host.
            sudo PYTHONDONTWRITEBYTECODE=1 \
            unshare --mount --pid --fork \
                $(exe {package_btrfs}) \
                    --subvolumes-dir "$subvolumes_dir" \
                    --output-path "$OUT" \
                    --opts $(location :{opts_name})
            '''.format(
                package_btrfs = antlir_dep("package:btrfs"),
                opts_name = opts_name,
            ),
            rule_type = _rule_type,
            target_name = name,
        ),
        visibility = visibility,
        labels = ["uses_sudo"] + (labels or []),
        antlir_rule = antlir_rule,
    )

btrfs = struct(
    new = _new_btrfs,
    opts = _btrfs_opts_api,
)
