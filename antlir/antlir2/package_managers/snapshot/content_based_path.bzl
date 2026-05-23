# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def should_use_content_based_path(actions: AnalysisActions, checksums: dict[str, str]) -> bool:
    digest_config = actions.digest_config()
    if digest_config.allows_sha1() and "sha1" in checksums:
        return True
    if digest_config.allows_sha256() and "sha256" in checksums:
        return True
    return False
