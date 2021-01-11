# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def fedora_storage_config(release):
    return {
        "bucket": "antlir",
        "key": "s3",
        "kind": "s3",
        "prefix": "snapshots/fedora/{}".format(release),
        "region": "us-east-2",
    }
