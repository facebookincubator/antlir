#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

loopdev="$ANTLIR2_LOOPDEV_0"

truncate -s 256M /image.btrfs
mkfs.btrfs /image.btrfs
mkdir -p /mnt/recv
mount /image.btrfs /mnt/recv -o loop="$loopdev"

pushd /mnt/recv

# NOTE: it's very important to test that receiving the child FAILS before
# receiving the parent, otherwise we could fool ourselves into believing that
# this works but in fact are relying on the parent subvolume still existing and
# being usable somewhere on the build host.
if btrfs receive -m /mnt/recv . < /child.sendstream; then
    echo "receive child should not have worked (yet)"
    exit 1
fi

btrfs receive -m /mnt/recv . < /parent.sendstream
mkdir child
pushd child
btrfs receive -m /mnt/recv . < /child.sendstream

cat volume/foo volume/bar
