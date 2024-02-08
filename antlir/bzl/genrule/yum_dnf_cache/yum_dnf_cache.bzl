# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Run `{dnf,yum} makecache` on each of the specified snapshots.

This is meant to be run on an image right after `install_rpm_repo_snapshot`.
For example, you should use this when defining a build appliance.

We cannot call `makecache` a part of building a standalone snapshot, because
a cache only makes sense in the context of a specific installer version,
which is determined by the image containing the snapshot.

WATCH OUT: This has no dedicated tests. But, it is tested:
  - `test_makecache` runs in all flavors of `test-yum-dnf-from-snapshot-*`.
  - `compiler/test_images:build_appliance_testing` uses this code.
  - `_check_no_repodata_fetches` in `test-repo-servers` ensures that the
    caches are actually used correctly.
Adding dedicated tests is probably not worth it because this is meant to be
used in the narrow context of constructing BA-like images, and the above
covers that use-case adequately.
"""

load("@bazel_skylib//lib:shell.bzl", "shell")
load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl:snapshot_install_dir.bzl", "snapshot_install_dir")

def image_yum_dnf_make_snapshot_cache(
        name,
        parent_layer,
        snapshot_to_installers,
        yum_is_dnf = False,
        **image_layer_kwargs):
    cmds = []
    for snapshot, installers in snapshot_to_installers.items():
        for prog in installers:
            cmds.append("{yum_dnf} makecache {maybe_fast}".format(
                yum_dnf = shell.quote(
                    snapshot_install_dir(snapshot) + "/{0}/bin/{0}".format(prog),
                ),
                # Plain `yum makecache` produces HUMONGOUS caches for no
                # obvious performance benefit.
                maybe_fast = "fast" if (prog == "yum" and not yum_is_dnf) else "",
            ))
    image.genrule_layer(
        name = name,
        cmd = ["/bin/bash", "-uec", ";".join(cmds)],
        parent_layer = parent_layer,
        rule_type = "yum_dnf_makecache_for_snapshot",
        user = "root",
        container_opts = struct(
            serve_rpm_snapshots = snapshot_to_installers.keys(),
            # We never call OS `yum/dnf` -- `cmd` invokes each wrapper in turn.
            shadow_proxied_binaries = False,
            # We need to write to the snapshots' directories.
            internal_only_unprotect_antlir_dir = True,
        ),
        **image_layer_kwargs
    )
