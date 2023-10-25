#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

loopdev="$ANTLIR2_LOOPDEV_0"

stat "$loopdev"

truncate -s 256M /image.btrfs
mkfs.btrfs /image.btrfs
mkdir /mnt/loop

mount /image.btrfs /mnt/loop -o loop="$loopdev"
