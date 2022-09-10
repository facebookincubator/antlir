#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import dataclasses
from typing import Any, Dict, List, Mapping, Optional, Set, Tuple, Union

from antlir.bzl_const import feature_for_layer
from antlir.common import get_logger
from antlir.compiler.items_for_features import (
    gen_included_features,
    GenFeaturesContext,
)
from antlir.find_built_subvol import find_built_subvol, Subvol
from antlir.fs_utils import Path

log = get_logger()


@dataclasses.dataclass(frozen=True)
class PackagedRoot:
    layer: Subvol
    layer_from_package: Dict[str, Any]


@dataclasses.dataclass(frozen=True)
class PathToRemove:
    path: str
    must_exist: bool


@dataclasses.dataclass
class ExtractedFeatures:
    packaged_root: Optional[PackagedRoot] = None
    make_dir_paths: Set[str] = dataclasses.field(default_factory=set)
    install_rpm_names: Set[str] = dataclasses.field(default_factory=set)
    features_needing_custom_image: Set[str] = dataclasses.field(
        default_factory=set
    )
    # Arguments to `gen_items_for_feature`.  This is a list if and only if
    # `features_needing_custom_image` is empty.
    features_to_replay: Optional[
        List[Tuple[str, str, Any]]
    ] = dataclasses.field(default_factory=list)
    paths_to_remove: Set[PathToRemove] = dataclasses.field(default_factory=set)

    def __iadd__(self, other: "ExtractedFeatures") -> "ExtractedFeatures":
        assert not (
            self.packaged_root and other.packaged_root
        ), f"Two root packages set: {self.packaged_root}, {other.packaged_root}"
        self.packaged_root = (
            self.packaged_root if self.packaged_root else other.packaged_root
        )
        self.make_dir_paths |= other.make_dir_paths
        self.install_rpm_names |= other.install_rpm_names
        self.features_needing_custom_image |= (
            other.features_needing_custom_image
        )
        self.paths_to_remove |= other.paths_to_remove
        # `other` gets populated by recursive `extract_nested_features`
        # calls, e.g. for `parent_layer`.
        # pyre-fixme[16]: `Optional` has no attribute `extend`.
        self.features_to_replay.extend(other.features_to_replay or [])
        return self


@dataclasses.dataclass(frozen=True)
class _FeatureHandlers:
    config: Dict[str, Any]
    layer_out: Path
    target_to_path: Mapping[str, Path]

    def layer_from_package(self) -> ExtractedFeatures:
        return ExtractedFeatures(
            packaged_root=PackagedRoot(
                layer=find_built_subvol(
                    self.layer_out,
                    path_in_repo=self.layer_out,
                ),
                layer_from_package=self.config,
            ),
        )

    def parent_layer(self) -> ExtractedFeatures:
        parent_layer_target = self.config["subvol"]
        project, parent_name = parent_layer_target.split(":", maxsplit=1)
        return extract_nested_features(
            layer_features_out=self.target_to_path[
                project + ":" + feature_for_layer(parent_name)
            ],
            layer_out=Path(self.target_to_path[parent_layer_target]),
            target_to_path=self.target_to_path,
        )

    def mounts(self) -> ExtractedFeatures:
        # We currently only support layer mounts in non-custom images, and a
        # mount is a layer mount IFF mount_config is None
        #
        # We don't **track** the mounts in `ExtractedFeatures` since they
        # can be retrieved using `mounts_from_meta`.
        return ExtractedFeatures(
            features_needing_custom_image={"mounts"}
            if self.config["mount_config"] is not None
            else set()
        )

    def rpms(self) -> ExtractedFeatures:
        if self.config.get("action") != "install":
            log.error(
                'RPM actions besides "install" need a custom image, got '
                f"{self.config}"
            )
            # pyre-fixme[7]: Expected `ExtractedFeatures` but got `None`.
            return None
        name = self.config.get("name")
        if name:
            names = {name}
        else:
            assert "source" in self.config, self.config
            # The names are only used for a sanity-check. We could potentially
            # add a dependency from the CM builder to the local RPMs, and
            # then extract their names, but building them is expensive, and
            # does not seem worthwhile for the sake of an assertion. Details:
            # fb.workplace.com/groups/btrmeup/permalink/3969652289781074
            log.error(
                "Installing an in-repo RPM requires a custom image (for now), "
                f" got {self.config}"
            )
            # pyre-fixme[7]: Expected `ExtractedFeatures` but got `None`.
            return None
        return ExtractedFeatures(install_rpm_names=names)

    def ensure_subdirs_exist(self) -> ExtractedFeatures:
        return ExtractedFeatures(
            make_dir_paths={
                (
                    Path(self.config["into_dir"])
                    / self.config["subdirs_to_create"]
                ).decode()
            },
        )

    def meta_key_value_store(self) -> ExtractedFeatures:
        # This is just metadata and doesn't need a custom image.
        return ExtractedFeatures()

    def remove_meta_key_value_store(self) -> ExtractedFeatures:
        # This is just metadata and doesn't need a custom image.
        # This is the same reasoning as `meta_key_value_store`.
        return ExtractedFeatures()

    def remove_paths(self) -> ExtractedFeatures:
        return ExtractedFeatures(
            paths_to_remove={
                PathToRemove(
                    path=self.config["path"],
                    must_exist=self.config["must_exist"],
                )
            }
        )


def extract_nested_features(
    *,
    layer_features_out: Path,
    layer_out: Path,
    target_to_path: Mapping[str, Path],
) -> ExtractedFeatures:
    extracted_features = ExtractedFeatures()
    for (feature_key, target, config) in gen_included_features(
        features_or_paths=[layer_features_out],
        features_ctx=GenFeaturesContext(
            target_to_path=target_to_path,
            subvolumes_dir=None,
            # We don't need to resolve all targets in the feature JSON
            ignore_missing_paths=True,
        ),
    ):
        non_custom_handler = getattr(
            _FeatureHandlers(
                config=config,
                layer_out=layer_out,
                target_to_path=target_to_path,
            ),
            feature_key,
            None,
        )
        non_custom_features = None
        if non_custom_handler:
            # This returns None if the handler thinks the features require a
            # custom image.  This hack would be avoided if we had separate
            # feature keys for "rpms_install" and "rpms_remove".
            non_custom_features = non_custom_handler()
        if non_custom_features:
            extracted_features += non_custom_features
        else:
            extracted_features += ExtractedFeatures(
                features_needing_custom_image={feature_key}
            )
        # pyre-fixme[16]: `Optional` has no attribute `append`.
        extracted_features.features_to_replay.append(
            (feature_key, target, config)
        )
    assert (
        extracted_features.packaged_root
    ), f"Root not set on extracted features {layer_features_out}"
    # Since extract_nested_features works on the pre-compiler, the items
    # are not ordered correctly. So we manually remove the directories
    for path_to_remove in extracted_features.paths_to_remove:
        if (
            path_to_remove.must_exist
            and path_to_remove.path not in extracted_features.make_dir_paths
        ):
            # If the directory to remove was not added by the user, we need a
            # custom image.
            extracted_features.features_needing_custom_image.add("remove_paths")
        extracted_features.make_dir_paths.discard(path_to_remove.path)
    extracted_features.paths_to_remove = set()

    # For custom images, replaying features is not supported (and will be a
    # bit tricky to support well), so make sure that any consumers that
    # accidentally try to do this will fail.
    if extracted_features.features_needing_custom_image:
        extracted_features.features_to_replay = None
    return extracted_features
