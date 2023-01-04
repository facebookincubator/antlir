#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Makes Items from the JSON that was produced by a Buck `feature` target"
import json
from typing import Any, Iterable, Mapping, NamedTuple, Optional, Union

from antlir.common import get_logger
from antlir.compiler.items.clone import CloneItem
from antlir.compiler.items.common import image_source_item, LayerOpts
from antlir.compiler.items.ensure_dirs_exist import ensure_subdirs_exist_factory
from antlir.compiler.items.genrule_layer import GenruleLayerItem
from antlir.compiler.items.group import GroupItem
from antlir.compiler.items.install_file import InstallFileItem
from antlir.compiler.items.make_subvol import LayerFromPackageItem, ParentLayerItem
from antlir.compiler.items.meta_key_value_store import (
    MetaKeyValueStoreItem,
    RemoveMetaKeyValueStoreItem,
)
from antlir.compiler.items.mount import MountItem
from antlir.compiler.items.remove_path import RemovePathItem
from antlir.compiler.items.requires import RequiresItem
from antlir.compiler.items.rpm_action import RpmActionItem
from antlir.compiler.items.symlink import SymlinkToDirItem, SymlinkToFileItem
from antlir.compiler.items.tarball import TarballItem
from antlir.compiler.items.user import UserItem, UsermodItem
from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import Path

log = get_logger()


class GenFeaturesContext(NamedTuple):
    target_to_path: Mapping[str, Path]
    subvolumes_dir: Optional[Path]
    ignore_missing_paths: bool


# Used alongside `ignore_missing_paths`
class UnknownTarget(NamedTuple):
    target: str


def replace_targets_by_paths(x: Any, ctx: GenFeaturesContext):
    """
    Converts target_tagger.bzl sigils to buck-out paths or Subvol objects.

    JSON-serialized image features store single-item dicts of the form
    {'__BUCK{_LAYER,}_TARGET': '//target:path'} whenever the compiler
    requires a path to another target.  This is because actual paths would
    break Buck caching, and would not survive repo moves.  Then, at runtime,
    the compiler receives a dictionary of target-to-path mappings as
    `--child-dependencies`, and performs the substitution in any image
    feature JSON it consumes.

    If `ctx.subvolumes_dir` is None, layer targets will not be replaced by
    their corresponding subvolumes, and will instead be left as-is.

    If `ctx.ignore_missing_paths` is True, the target will simply be
    returned if it is not found in `target_to_path`.
    """
    if type(x) is dict:
        if "__BUCK_TARGET" in x or "__BUCK_LAYER_TARGET" in x:
            assert len(x) == 1, x
            ((sigil, target),) = x.items()
            if sigil == "__BUCK_LAYER_TARGET" and ctx.subvolumes_dir is None:
                return target  # pragma: no cover
            path = ctx.target_to_path.get(target)
            if not path:
                if ctx.ignore_missing_paths:  # pragma: no cover
                    return UnknownTarget(target)
                raise RuntimeError(f"{target} not in {ctx.target_to_path}")
            return (
                path
                if sigil == "__BUCK_TARGET"
                else find_built_subvol(path, subvolumes_dir=ctx.subvolumes_dir)
            )
        return {k: replace_targets_by_paths(v, ctx) for k, v in x.items()}
    elif type(x) is list:
        return [replace_targets_by_paths(v, ctx) for v in x]
    elif type(x) in [int, float, str, bool, type(None)]:
        return x
    raise AssertionError(f"Unknown {type(x)} for {x}")  # pragma: no cover


def gen_included_features(
    features_or_paths: Iterable[Union[str, dict, Path]],
    features_ctx: GenFeaturesContext,
):
    for feature_or_path in features_or_paths:
        if isinstance(feature_or_path, Path) or isinstance(feature_or_path, str):
            with open(feature_or_path) as f:
                items = replace_targets_by_paths(json.load(f), features_ctx)
        else:
            # Any inline feature would have had its target paths unwrapped by
            # the outer feature that contains it.  That's always true because
            # the compiler gets the root features on the command line as paths
            # to JSON.
            items = feature_or_path

        yield from gen_included_features(
            features_or_paths=items.pop("features", []),
            features_ctx=features_ctx,
        )

        target = items.pop("target")
        for feature_key, configs in items.items():
            for cfg in configs:
                yield (feature_key, target, cfg)


class ItemFactory:
    def __init__(self, layer_opts: LayerOpts) -> None:
        self._layer_opts = layer_opts
        self._key_to_item_factory = {
            "clone": self._image_sourcify(CloneItem),
            "genrule_layer": GenruleLayerItem,
            "groups": GroupItem,
            "install_files": self._image_sourcify(InstallFileItem),
            "layer_from_package": self._image_sourcify(LayerFromPackageItem),
            "mounts": lambda **kwargs: MountItem(**kwargs, layer_opts=layer_opts),
            "parent_layer": ParentLayerItem,
            "remove_paths": RemovePathItem,
            "rpms": self._image_sourcify(
                lambda **kwargs: RpmActionItem(**kwargs, layer_opts=layer_opts)
            ),
            "symlinks_to_dirs": SymlinkToDirItem,
            "symlinks_to_files": SymlinkToFileItem,
            "tarballs": self._image_sourcify(TarballItem),
            "users": UserItem,
            "usermod": UsermodItem,
            "requires": RequiresItem,
            "meta_key_value_store": MetaKeyValueStoreItem,
            "remove_meta_key_value_store": RemoveMetaKeyValueStoreItem,
        }
        self._key_to_items_factory = {
            "ensure_subdirs_exist": ensure_subdirs_exist_factory,
        }

    def _image_sourcify(self, item_cls):
        return image_source_item(item_cls, layer_opts=self._layer_opts)

    def gen_items_for_feature(self, feature_key: str, target: str, config):
        if feature_key in self._key_to_item_factory:
            yield self._key_to_item_factory[feature_key](from_target=target, **config)
        elif feature_key in self._key_to_items_factory:
            yield from self._key_to_items_factory[feature_key](
                from_target=target, **config
            )
        else:  # pragma: no cover
            raise AssertionError(f"Unsupported item: {feature_key}")


def gen_items_for_features(
    *,
    features_or_paths: Iterable[Union[str, dict, Path]],
    layer_opts: LayerOpts,
):
    factory = ItemFactory(layer_opts)
    for key_target_config in gen_included_features(
        features_or_paths=features_or_paths,
        features_ctx=GenFeaturesContext(
            target_to_path=layer_opts.target_to_path,
            subvolumes_dir=layer_opts.subvolumes_dir,
            ignore_missing_paths=False,
        ),
    ):
        try:
            yield from factory.gen_items_for_feature(*key_target_config)
        except Exception:  # pragma: no cover
            log.error(f"While constructing image feature {key_target_config}")
            raise
