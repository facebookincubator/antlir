#!/bin/sh
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e
set -x

mount -t proc none /proc
mount -t sysfs none /sys

mdev -s

NEWROOT="/newroot"

mount -o subvol=volume -t btrfs /dev/vda "$NEWROOT"

exec switch_root "$NEWROOT" /sbin/init
