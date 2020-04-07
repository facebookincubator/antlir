#!/bin/sh
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e
set -x

mkdir /proc
mkdir /sys

mount -t proc none /proc
mount -t sysfs none /sys

# newer kernels have VIRTIO_BLK=y, so only load this if we have it as a module
if [ -f "/modules/kernel/drivers/block/virtio_blk.ko" ]; then
  insmod "/modules/kernel/drivers/block/virtio_blk.ko"
fi

mdev -s

NEWROOT="/newroot"
mkdir "$NEWROOT"

mount -o subvol=volume -t btrfs /dev/vda "$NEWROOT"

# make the new root writable using 'btrfs device add'
# TODO(T62846368): this requires that the root image has btrfs-progs installed,
# which is currently always the case, but should be automatically added to any
# arbitrary user-provided image.layer when that is the only API to use vmtest
cd "$NEWROOT"
mount -t proc proc proc/
mount --rbind /sys sys/
mount --rbind /dev dev/
chroot "$NEWROOT" /sbin/btrfs device add /dev/vdb /
umount proc sys dev

mount -o remount,rw "$NEWROOT"

# Copy all the modules we have into the root disk. This allows us to have some
# modules recompiled for older kernels that did not build with them (for
# example 9p in older kernels) which are not installed in the root fs.
if [ -d "/modules/kernel" ]; then
  cp -R /modules "$NEWROOT/lib/modules/$(uname -r)"
  chroot "$NEWROOT" /sbin/depmod
fi

exec switch_root "$NEWROOT" /sbin/init
