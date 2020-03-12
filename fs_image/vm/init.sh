#!/bin/sh
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

/bin/seedroot

mount -o remount,rw "$NEWROOT"

# Copy all the modules we have into the root disk. This allows us to have some
# modules recompiled for older kernels that did not build with them (for
# example 9p in older kernels) which are not installed in the root fs.
if [ -d "/modules/kernel" ]; then
  cp -R /modules "$NEWROOT/lib/modules/$(uname -r)"
  chroot "$NEWROOT" /sbin/depmod
fi

exec switch_root "$NEWROOT" /sbin/init
