# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

DmVerityInfo = provider(
    fields = {
        "data_block_size": int,
        "data_image": Artifact,
        "hash_algorithm": str,
        "hash_block_size": int,
        "hash_image": Artifact,
        "image": Artifact,
        "root_hash": Artifact,
        "salt": str,
    },
    doc = "Dm-verity wrapped image: combined image, separate hash image, root hash, and hashing parameters.",
)
