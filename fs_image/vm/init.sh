#!/bin/sh
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e
set -x

mount -t proc none /proc
mount -t sysfs none /sys

# newer kernels have VIRTIO_BLK=y, but load it for kernels that ship it as a
# module which is then copied into the initrd
if [ -f "/lib/modules/$(uname -r)/kernel/drivers/block/virtio_blk.ko" ]; then
    insmod "/lib/modules/$(uname -r)/kernel/drivers/block/virtio_blk.ko"
fi

mdev -s

NEWROOT="/newroot"

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

# Mount (most) modules over 9p fs share, because they are not installed into
# the root fs. There are some kernel modules that are built into the initrd (at
# a minimum 9p and 9pnet_virtio) based on kernel version, but all modules are
# available under this 9p mount
mkdir -p "$NEWROOT/lib/modules/$(uname -r)/kernel"
modprobe 9pnet
modprobe 9pnet_virtio
modprobe 9p
mount -t 9p -o trans=virtio,version=9p2000.L,cache=loose,posixacl kernel-modules "$NEWROOT/lib/modules/$(uname -r)/kernel"

# We cannot run depmod at build time, because we need to mount only the
# `kernel/` directory of /lib/modules/$rel, because bpf tests require the
# /lib/modules/$rel/build symlink, and buck fails when there are broken
# symlinks in an output directory
chroot "$NEWROOT" /sbin/depmod

exec switch_root "$NEWROOT" /sbin/init
