#!/usr/bin/env python3
'''
This is normally invoked by the `image_layer` Buck macro converter.

This compiler builds a btrfs subvolume in
  <--subvolumes-dir>/<--subvolume-rel-path>

To do so, it parses `--child-feature-json` and the `--child-dependencies`
that referred therein, creates `ImageItems`, sorts them in dependency order,
and invokes `.build()` to apply each item to actually construct the subvol.
'''

import argparse
import os
import sys

from contextlib import ExitStack

from compiler.items_for_features import gen_items_for_features
from fs_image.compiler.items.common import LayerOpts
from fs_image.compiler.items.phases_provide import PhasesProvideItem
from subvol_utils import Subvol, get_subvolume_path

from .dep_graph import DependencyGraph
from .subvolume_on_disk import SubvolumeOnDisk


# At the moment, the target names emitted by `image_feature` targets seem to
# be normalized the same way as those provided to us by `image_layer`.  If
# this were to ever change, this would be a good place to re-normalize them.
def make_target_path_map(targets_followed_by_paths):
    'Buck query_targets_and_outputs gives us `//target path/to/target/out`'
    if len(targets_followed_by_paths) % 2 != 0:
        raise RuntimeError(
            f'Odd-length --child-dependencies {targets_followed_by_paths}'
        )
    it = iter(targets_followed_by_paths)
    d = dict(zip(it, it))
    return d


def parse_args(args) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawTextHelpFormatter,
    )
    parser.add_argument(
        '--subvolumes-dir', required=True,
        help='A directory on a btrfs volume to store the compiled subvolume '
            'representing the new layer',
    )
    # We separate this from `--subvolumes-dir` in order to help keep our
    # JSON output ignorant of the absolute path of the repo.
    parser.add_argument(
        '--subvolume-rel-path', required=True,
        help='Path underneath --subvolumes-dir where we should create '
            'the subvolume. Note that all path components but the basename '
            'should already exist.',
    )
    parser.add_argument(
        '--yum-from-repo-snapshot',
        help='Path to a binary taking `--install-root PATH -- SOME YUM ARGS`.',
    )
    parser.add_argument(
        '--build-appliance-json',
        help='Path to the JSON output of target referred by build_appliance',
    )
    parser.add_argument(
        '--preserve-yum-cache', action='store_true',
        help='RpmAction preserves /var/cache/yum doing yum install. For now, '
             'if this option is omitted and --build-appliance-json is not '
             'provided, /var/cache/yum will be preserved anyway.',
    )
    parser.add_argument(
        '--artifacts-may-require-repo', action='store_true',
        help='Buck @mode/dev produces "in-place" build artifacts that are '
            'not truly standalone. It is important to be able to execute '
            'code from images built in this mode to support rapid '
            'development and debugging, even though it is not a "true" '
            'self-contained image. To allow execution of in-place binaries, '
            'fs_image runtimes will automatically mount the repo into any '
            '`--artifacts-may-require-repo` image at runtime (e.g. when '
            'running image unit-tests, when using `-container` or `-boot` '
            'targets, when using the image as a build appliance).',
    )
    parser.add_argument(
        '--child-layer-target', required=True,
        help='The name of the Buck target describing the layer being built',
    )
    parser.add_argument(
        '--child-feature-json', action='append', default=[],
        help='The path of the JSON output of any `image.feature`s that are'
            'directly included by the layer being built',
    )
    parser.add_argument(
        '--child-dependencies',
        nargs=argparse.REMAINDER, metavar=['TARGET', 'PATH'], default=(),
        help='Consumes the remaining arguments on the command-line, with '
            'arguments at positions 1, 3, 5, 7, ... used as Buck target names '
            '(to be matched with the targets in per-feature JSON outputs). '
            'The argument immediately following each target name must be a '
            'path to the output of that target on disk.',
    )
    return parser.parse_args(args)


def build_image(args):
    subvol = Subvol(os.path.join(args.subvolumes_dir, args.subvolume_rel_path))
    layer_opts = LayerOpts(
        layer_target=args.child_layer_target,
        yum_from_snapshot=args.yum_from_repo_snapshot,
        build_appliance=get_subvolume_path(
            args.build_appliance_json, args.subvolumes_dir,
        ) if args.build_appliance_json else None,
        preserve_yum_cache=args.preserve_yum_cache,
        artifacts_may_require_repo=args.artifacts_may_require_repo,
        target_to_path=make_target_path_map(args.child_dependencies),
        subvolumes_dir=args.subvolumes_dir,
    )

    # This stack allows build items to hold temporary state on disk.
    with ExitStack() as exit_stack:
        dep_graph = DependencyGraph(gen_items_for_features(
            exit_stack=exit_stack,
            features_or_paths=args.child_feature_json,
            layer_opts=layer_opts,
        ), layer_target=args.child_layer_target)
        # Creating all the builders up-front lets phases validate their input
        for builder in [
            builder_maker(items, layer_opts)
                for builder_maker, items in dep_graph.ordered_phases()
        ]:
            builder(subvol)
        # We cannot validate or sort `ImageItem`s until the phases are
        # materialized since the items may depend on the output of the phases.
        for item in dep_graph.gen_dependency_order_items(PhasesProvideItem(
            from_target=args.child_layer_target,
            subvol=subvol,
        )):
            item.build(subvol, layer_opts)
        # Build artifacts should never change. Run this BEFORE the exit_stack
        # cleanup to enforce that the cleanup does not touch the image.
        subvol.set_readonly(True)

    try:
        return SubvolumeOnDisk.from_subvolume_path(
            # Converting to a path here does not seem too risky since this
            # class shouldn't have a reason to follow symlinks in the subvol.
            subvol.path().decode(),
            args.subvolumes_dir,
        )
    # The complexity of covering this is high, but the only thing that can
    # go wrong is a typo in the f-string.
    except Exception as ex:  # pragma: no cover
        raise RuntimeError(f'Serializing subvolume {subvol.path()}') from ex


if __name__ == '__main__':  # pragma: no cover
    build_image(parse_args(sys.argv[1:])).to_json_file(sys.stdout)
