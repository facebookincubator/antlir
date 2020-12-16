#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Makes Items from the JSON that was produced by the Buck target image_feature"
import json
from typing import Iterable, Union, Mapping, Optional

from antlir.compiler.items.clone import CloneItem
from antlir.compiler.items.common import LayerOpts, image_source_item
from antlir.compiler.items.foreign_layer import ForeignLayerItem
from antlir.compiler.items.install_file import InstallFileItem
from antlir.compiler.items.make_dirs import MakeDirsItem
from antlir.compiler.items.make_subvol import (
    ParentLayerItem,
    ReceiveSendstreamItem,
)
from antlir.compiler.items.mount import MountItem
from antlir.compiler.items.remove_path import RemovePathItem
from antlir.compiler.items.rpm_action import RpmActionItem
from antlir.compiler.items.symlink import SymlinkToDirItem, SymlinkToFileItem
from antlir.compiler.items.tarball import TarballItem
from antlir.find_built_subvol import find_built_subvol


def replace_targets_by_paths(
    x,
    target_to_path: Mapping[str, str],
    subvolumes_dir: Optional[str],
):
    """
    Converts target_tagger.bzl sigils to buck-out paths or Subvol objects.

    JSON-serialized image features store single-item dicts of the form
    {'__BUCK{_LAYER,}_TARGET': '//target:path'} whenever the compiler
    requires a path to another target.  This is because actual paths would
    break Buck caching, and would not survive repo moves.  Then, at runtime,
    the compiler receives a dictionary of target-to-path mappings as
    `--child-dependencies`, and performs the substitution in any image
    feature JSON it consumes.

    Note: If `subvolumes_dir` is None, layer targets will not be replaced by
    their corresponding subvolumes, and will instead be left as-is.
    """
    if type(x) is dict:
        if "__BUCK_TARGET" in x or "__BUCK_LAYER_TARGET" in x:
            assert len(x) == 1, x
            ((sigil, target),) = x.items()
            if sigil == "__BUCK_LAYER_TARGET" and subvolumes_dir is None:
                return target  # pragma: no cover
            path = target_to_path.get(target)
            if not path:
                raise RuntimeError(f"{target} not in {target_to_path}")
            return (
                path
                if sigil == "__BUCK_TARGET"
                else find_built_subvol(path, subvolumes_dir=subvolumes_dir)
            )
        return {
            k: replace_targets_by_paths(v, target_to_path, subvolumes_dir)
            for k, v in x.items()
        }
    elif type(x) is list:
        return [
            replace_targets_by_paths(v, target_to_path, subvolumes_dir)
            for v in x
        ]
    elif type(x) in [int, float, str, bool, type(None)]:
        return x
    raise AssertionError(f"Unknown {type(x)} for {x}")  # pragma: no cover


def gen_included_features(
    features_or_paths: Iterable[Union[str, dict]],
    target_to_path: Mapping[str, str],
    subvolumes_dir: Optional[str],
):
    for feature_or_path in features_or_paths:
        if isinstance(feature_or_path, str):
            with open(feature_or_path) as f:
                items = replace_targets_by_paths(
                    json.load(f),
                    target_to_path,
                    subvolumes_dir,
                )
        else:
            # Any inline feature would have had its target paths unwrapped by
            # the outer feature that contains it.  That's always true because
            # the compiler gets the root features on the command line as paths
            # to JSON.
            items = feature_or_path

        yield from gen_included_features(
            features_or_paths=items.pop("features", []),
            target_to_path=target_to_path,
            subvolumes_dir=subvolumes_dir,
        )

        target = items.pop("target")
        for feature_key, configs in items.items():
            yield (feature_key, target, configs)


def gen_items_for_features(
    *,
    exit_stack,
    features_or_paths: Iterable[Union[str, dict]],
    layer_opts: LayerOpts,
):
    def image_sourcify(item_cls):
        return image_source_item(
            item_cls, exit_stack=exit_stack, layer_opts=layer_opts
        )

    key_to_item_factory = {
        "clone": image_sourcify(CloneItem),
        "install_files": image_sourcify(InstallFileItem),
        "make_dirs": MakeDirsItem,
        "mounts": lambda **kwargs: MountItem(**kwargs, layer_opts=layer_opts),
        "parent_layer": ParentLayerItem,
        "rpms": image_sourcify(RpmActionItem),
        "remove_paths": RemovePathItem,
        "symlinks_to_dirs": SymlinkToDirItem,
        "symlinks_to_files": SymlinkToFileItem,
        "tarballs": image_sourcify(TarballItem),
        "receive_sendstreams": image_sourcify(ReceiveSendstreamItem),
        "foreign_layer": ForeignLayerItem,
    }

    for (feature_key, target, configs) in gen_included_features(
        features_or_paths=features_or_paths,
        target_to_path=layer_opts.target_to_path,
        subvolumes_dir=layer_opts.subvolumes_dir,
    ):
        assert (
            feature_key in key_to_item_factory
        ), f"Unsupported item: {feature_key}"
        item_factory = key_to_item_factory[feature_key]
        for config in configs:
            yield item_factory(from_target=target, **config)
