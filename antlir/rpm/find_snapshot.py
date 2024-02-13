#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import base64
import hashlib

from antlir.fs_utils import Path


# KEEP IN SYNC with their copies in `bzl/snapshot_install_dir.bzl`
RPM_SNAPSHOT_BASE_DIR = Path("/__antlir__/rpm/repo-snapshot")


def _sha256_b64(b: bytes) -> str:
    return base64.urlsafe_b64encode(hashlib.sha256(b).digest()).strip(b"=").decode()


# KEEP IN SYNC with its copy in `bzl/wrap_target.bzl`.
def abbrev_name(name: str, min_abbrev: int) -> str:
    return (
        name
        if len(name) < (2 * min_abbrev + 3)
        else (name[:min_abbrev] + "..." + name[-min_abbrev:])
    )


# KEEP IN SYNC with its copy in `bzl/wrap_target.bzl`.
def mangle_target(normalized_target: str, min_abbrev: int = 15) -> str:
    "The docs are on the other copy of this function in `target_tagger.bzl`."
    _, name = normalized_target.split(":")
    return (
        abbrev_name(name, min_abbrev)
        + f"__{_sha256_b64(normalized_target.encode())[:20]}"
    )


# KEEP IN SYNC with its copy in `bzl/snapshot_install_dir.bzl`
def snapshot_install_dir(snapshot: str):
    if ":" in snapshot:
        # remove various suffixes from the snapshot target
        path, tgt = snapshot.split(":")
        if ".rc" in tgt:
            tgt = tgt.split(".rc")[0]
        if tgt.endswith(".layer"):
            tgt = tgt[: -len(".layer")]
        snapshot = path + ":" + tgt
    return RPM_SNAPSHOT_BASE_DIR / mangle_target(snapshot)
