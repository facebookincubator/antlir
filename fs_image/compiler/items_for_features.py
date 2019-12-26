#!/usr/bin/env python3
'Makes Items from the JSON that was produced by the Buck target image_feature'
import json

from typing import Iterable, Union

from find_built_subvol import find_built_subvol

from fs_image.compiler.items.common import LayerOpts, image_source_item
from fs_image.compiler.items.install_file import InstallFileItem
from fs_image.compiler.items.make_dirs import MakeDirsItem
from fs_image.compiler.items.make_subvol import (
    ParentLayerItem, ReceiveSendstreamItem,
)
from fs_image.compiler.items.mount import MountItem
from fs_image.compiler.items.remove_path import RemovePathItem
from fs_image.compiler.items.rpm_action import RpmActionItem
from fs_image.compiler.items.rpm_build import RpmBuildItem
from fs_image.compiler.items.symlink import SymlinkToDirItem, SymlinkToFileItem
from fs_image.compiler.items.tarball import TarballItem


def replace_targets_by_paths(x, layer_opts: LayerOpts):
    '''
    Converts target_tagger.bzl sigils to buck-out paths or Subvol objects.

    JSON-serialized image features store single-item dicts of the form
    {'__BUCK{_LAYER,}_TARGET': '//target:path'} whenever the compiler
    requires a path to another target.  This is because actual paths would
    break Buck caching, and would not survive repo moves.  Then, at runtime,
    the compiler receives a dictionary of target-to-path mappings as
    `--child-dependencies`, and performs the substitution in any image
    feature JSON it consumes.
    '''
    if type(x) is dict:
        if '__BUCK_TARGET' in x or '__BUCK_LAYER_TARGET' in x:
            assert len(x) == 1, x
            (sigil, target), = x.items()
            path = layer_opts.target_to_path.get(target)
            if not path:
                raise RuntimeError(
                    f'{target} not in {layer_opts.target_to_path}'
                )
            return path if sigil == '__BUCK_TARGET' else find_built_subvol(
                path, subvolumes_dir=layer_opts.subvolumes_dir,
            )
        return {
            k: replace_targets_by_paths(v, layer_opts) for k, v in x.items()
        }
    elif type(x) is list:
        return [replace_targets_by_paths(v, layer_opts) for v in x]
    elif type(x) in [int, float, str, bool, type(None)]:
        return x
    raise AssertionError(f'Unknown {type(x)} for {x}')  # pragma: no cover


def gen_items_for_features(
    *, exit_stack, features_or_paths: Iterable[Union[str, dict]],
    layer_opts: LayerOpts,
):
    def image_sourcify(item_cls):
        return image_source_item(
            item_cls, exit_stack=exit_stack, layer_opts=layer_opts,
        )

    key_to_item_factory = {
        'install_files': image_sourcify(InstallFileItem),
        'make_dirs': MakeDirsItem,
        'mounts': MountItem,
        'parent_layer': ParentLayerItem,
        'rpms': image_sourcify(RpmActionItem),
        'remove_paths': RemovePathItem,
        'symlinks_to_dirs': SymlinkToDirItem,
        'symlinks_to_files': SymlinkToFileItem,
        'tarballs': image_sourcify(TarballItem),
        'receive_sendstreams': image_sourcify(ReceiveSendstreamItem),
        'rpm_build': RpmBuildItem,
    }

    for feature_or_path in features_or_paths:
        if isinstance(feature_or_path, str):
            with open(feature_or_path) as f:
                items = replace_targets_by_paths(json.load(f), layer_opts)
        else:
            # An inline features would have had its target paths unwrapped
            # by the outer feature that contains it.  That's always true
            # because the compiler gets the root features on the command
            # line as paths to JSON.
            items = feature_or_path

        yield from gen_items_for_features(
            exit_stack=exit_stack,
            features_or_paths=items.pop('features', []),
            layer_opts=layer_opts,
        )

        target = items.pop('target')
        for key, item_factory in key_to_item_factory.items():
            for dct in items.pop(key, []):
                try:
                    yield item_factory(from_target=target, **dct)
                except Exception as ex:  # pragma: no cover
                    raise RuntimeError(
                        f'Failed to process {key}: {dct} from target '
                        f'{target}, please read the exception above.'
                    ) from ex

        assert not items, f'Unsupported items: {items}'
