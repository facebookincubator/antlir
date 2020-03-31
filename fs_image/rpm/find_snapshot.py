#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import binascii

from fs_image.fs_utils import Path


# KEEP IN SYNC with its copy in `bzl/rpm_repo_snapshot.bzl`
RPM_SNAPSHOT_BASE_DIR = Path('/__fs_image__/rpm-repo-snapshot')


# KEEP IN SYNC with its copy in `bzl/target_tagger.bzl`.
def mangle_target(normalized_target: str, min_abbrev: int = 15) -> str:
    'The docs are on the other copy of this function in `target_tagger.bzl`.'
    _, name = normalized_target.split(":")
    return (
        name if len(name) < (2 * min_abbrev + 3) else (
            name[:min_abbrev] + "..." + name[-min_abbrev:]
        )
    ) + f'__{binascii.crc32(normalized_target.encode()):x}'


# KEEP IN SYNC with its copy in `bzl/rpm_repo_snapshot.bzl`
def snapshot_install_dir(snapshot):
    return RPM_SNAPSHOT_BASE_DIR / mangle_target(snapshot)
