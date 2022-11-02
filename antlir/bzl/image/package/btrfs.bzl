# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:bash.bzl", "wrap_bash_build_in_common_boilerplate")
load("//antlir/bzl:loopback_opts.bzl", "normalize_loopback_opts")
load("//antlir/bzl:oss_shim.bzl", "buck_genrule")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load(":btrfs.shape.bzl", "btrfs_opts_t", "btrfs_subvol_t")

def _new_btrfs_subvol(**kwargs):
    return btrfs_subvol_t(
        **kwargs
    )

_btrfs_subvol_api = struct(
    new = _new_btrfs_subvol,
    t = btrfs_subvol_t,
)

def _new_btrfs_opts(subvols, default_subvol = None, loopback_opts = None, **kwargs):
    if default_subvol and not default_subvol.startswith("/"):
        fail("Default subvol must be an absolute path: " + default_subvol)

    loopback_opts = normalize_loopback_opts(loopback_opts)
    if structs.to_dict(loopback_opts).get("size_mb", None) != None:
        fail(
            "The 'size_mb' parameter is not supported for btrfs packages." +
            " Use 'free_mb' instead.",
        )

    return btrfs_opts_t(
        subvols = subvols,
        default_subvol = default_subvol,
        loopback_opts = loopback_opts,
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

    buck_genrule(
        name = name,
        out = "image.btrfs",
        type = _rule_type,
        bash = wrap_bash_build_in_common_boilerplate(
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
                    --subvolumes-dir "$SUBVOLUMES_DIR" \
                    --output-path "$OUT" \
                    --opts {quoted_opts_json}
            '''.format(
                package_btrfs = antlir_dep("package:btrfs"),
                quoted_opts_json = shell.quote(shape.do_not_cache_me_json(opts)),
            ),
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
