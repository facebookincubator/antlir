#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This is normally invoked by the `image_layer` Buck macro converter.

This compiler builds a btrfs subvolume in
  <--subvolumes-dir>/<--subvolume-rel-path>

To do so, it parses `--child-feature-json` and the `--child-dependencies`
that referred therein, creates `ImageItems`, sorts them in dependency order,
and invokes `.build()` to apply each item to actually construct the subvol.
"""

import argparse
import os
import stat
import sys
from contextlib import ExitStack
from typing import Iterator

from antlir.cli import add_targets_and_outputs_arg
from antlir.compiler.items.common import LayerOpts
from antlir.compiler.items.phases_provide import PhasesProvideItem
from antlir.compiler.items_for_features import gen_items_for_features
from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import META_FLAVOR_FILE, Path
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.subvol_utils import Subvol

from .dep_graph import DependencyGraph, ImageItem
from .subvolume_on_disk import SubvolumeOnDisk


def parse_args(args) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawTextHelpFormatter
    )
    parser.add_argument(
        "--subvolumes-dir",
        required=True,
        type=Path.from_argparse,
        help="A directory on a btrfs volume to store the compiled subvolume "
        "representing the new layer",
    )
    # We separate this from `--subvolumes-dir` in order to help keep our
    # JSON output ignorant of the absolute path of the repo.
    parser.add_argument(
        "--subvolume-rel-path",
        required=True,
        type=Path.from_argparse,
        help="Path underneath --subvolumes-dir where we should create "
        "the subvolume. Note that all path components but the basename "
        "should already exist.",
    )
    parser.add_argument(
        "--build-appliance-buck-out",
        type=Path.from_argparse,
        help="Path to the Buck output of the build appliance target",
    )
    parser.add_argument(
        "--rpm-installer",
        type=YumDnf,
        help="Name of a supported RPM package manager (e.g. `yum` or `dnf`). "
        "Required if your image installs RPMs.",
    )
    parser.add_argument(
        "--rpm-repo-snapshot",
        type=Path.from_argparse,
        help="Path to snapshot directory in the build appliance image. "
        "The default is the BA symlink for `--rpm-installer` under "
        "`RPM_DEFAULT_SNAPSHOT_FOR_INSTALLER_DIR`.",
    )
    parser.add_argument(
        "--artifacts-may-require-repo",
        action="store_true",
        help='Buck @mode/dev produces "in-place" build artifacts that are '
        "not truly standalone. It is important to be able to execute "
        "code from images built in this mode to support rapid "
        'development and debugging, even though it is not a "true" '
        "self-contained image. To allow execution of in-place binaries, "
        "antlir runtimes will automatically mount the repo into any "
        "`--artifacts-may-require-repo` image at runtime (e.g. when "
        "running image unit-tests, when using `=container` or `=systemd` "
        "targets, when using the image as a build appliance).",
    )
    parser.add_argument(
        "--child-layer-target",
        required=True,
        help="The name of the Buck target describing the layer being built",
    )
    parser.add_argument(
        "--child-feature-json",
        action="append",
        default=[],
        help="The path of the JSON output of any `feature`s that are"
        "directly included by the layer being built",
    )
    parser.add_argument("--debug", action="store_true", help="Log more")
    parser.add_argument(
        "--allowed-host-mount-target",
        action="append",
        default=[],
        help="Target name that is allowed to contain host mounts used as "
        "build_sources.  Can be specified more than once.",
    )
    parser.add_argument(
        "--version-set-override",
        help="Path to a file containing TAB-separated ENVRAs, one per line."
        "Also refer to `build_opts.bzl`.",
    )
    parser.add_argument(
        "--flavor",
        required=True,
        help="The flavor of the image that will be written into `/.meta` "
        "directory.",
    )
    parser.add_argument(
        "--unsafe-bypass-flavor-check",
        action="store_true",
        help="Do NOT use this",
    )

    add_targets_and_outputs_arg(parser)
    return Path.parse_args(parser, args)


def compile_items_to_subvol(
    *,
    exit_stack: ExitStack,
    subvol: Subvol,
    layer_opts: LayerOpts,
    iter_items: Iterator[ImageItem],
) -> None:
    dep_graph = DependencyGraph(
        iter_items=iter_items,
        layer_target=layer_opts.layer_target,
    )
    # Creating all the builders up-front lets phases validate their input
    for builder in [
        builder_maker(items, layer_opts)
        for builder_maker, items in dep_graph.ordered_phases()
    ]:
        builder(subvol)
    # We cannot validate or sort `ImageItem`s until the phases are
    # materialized since the items may depend on the output of the phases.
    for item in dep_graph.gen_dependency_order_items(
        PhasesProvideItem(from_target=layer_opts.layer_target, subvol=subvol)
    ):
        # pyre-fixme[16]: `ImageItem` has no attribute `build`.
        item.build(subvol, layer_opts)


def build_image(args):
    # We want check the umask since it can affect the result of the
    # `os.access` check for `image.install*` items.  That said, having a
    # umask that denies execute permission to "user" is likely to break this
    # code earlier, since new directories wouldn't be traversible.  At least
    # this check gives a nice error message.
    cur_umask = os.umask(0)
    os.umask(cur_umask)
    assert (
        cur_umask & stat.S_IXUSR == 0
    ), f"Refusing to run with pathological umask 0o{cur_umask:o}"

    subvol = Subvol(args.subvolumes_dir / args.subvolume_rel_path)

    build_appliance = None
    if args.build_appliance_buck_out:
        build_appliance = find_built_subvol(
            args.build_appliance_buck_out, subvolumes_dir=args.subvolumes_dir
        )

    layer_opts = LayerOpts(
        layer_target=args.child_layer_target,
        build_appliance=build_appliance,
        rpm_installer=args.rpm_installer,
        rpm_repo_snapshot=args.rpm_repo_snapshot,
        artifacts_may_require_repo=args.artifacts_may_require_repo,
        target_to_path=args.targets_and_outputs,
        subvolumes_dir=args.subvolumes_dir,
        version_set_override=args.version_set_override,
        debug=args.debug,
        allowed_host_mount_targets=frozenset(args.allowed_host_mount_target),
        flavor=args.flavor,
        unsafe_bypass_flavor_check=args.unsafe_bypass_flavor_check,
    )

    # This stack allows build items to hold temporary state on disk.
    with ExitStack() as exit_stack:
        compile_items_to_subvol(
            exit_stack=exit_stack,
            subvol=subvol,
            layer_opts=layer_opts,
            iter_items=gen_items_for_features(
                exit_stack=exit_stack,
                features_or_paths=args.child_feature_json,
                layer_opts=layer_opts,
            ),
        )
        # Build artifacts should never change. Run this BEFORE the exit_stack
        # cleanup to enforce that the cleanup does not touch the image.
        subvol.set_readonly(True)

    try:
        return SubvolumeOnDisk.from_subvolume_path(
            # Converting to a path here does not seem too risky since this
            # class shouldn't have a reason to follow symlinks in the subvol.
            subvol.path(),
            args.subvolumes_dir,
            build_appliance.path() if build_appliance else None,
        )
    # The complexity of covering this is high, but the only thing that can
    # go wrong is a typo in the f-string.
    except Exception as ex:  # pragma: no cover
        raise RuntimeError(f"Serializing subvolume {subvol.path()}") from ex


if __name__ == "__main__":  # pragma: no cover
    from antlir.common import init_logging

    args = parse_args(sys.argv[1:])
    init_logging(debug=args.debug)
    build_image(args).to_json_file(sys.stdout)
