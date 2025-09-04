#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

if [ ! -d "antlir2-out" ]; then
    cd "$(hg root)"
fi

if [ ! -d "antlir2-out" ]; then
    echo "no antlir2-out found in repo root, exiting..."
    exit
fi

pushd "antlir2-out"

# Quick pass to just delete all the top-level subvolumes. Some of these might
# fail if there are nested subvols but that's ok
sudo find . -maxdepth 2 -type d -inum 256 -exec btrfs subvolume delete {} \; || true

# all top-level subvolumes must be made read-write in case any nested
# subvolume needs to be deleted (the compiler will now loudly fail if these are
# found, but we still want to clean them up)
# subvolumes may be found in either antlir2-out (old style) or
# antlir2-out/subvols (new style) hence the -maxdepth 2
find . -maxdepth 2 -depth -type d -inum 256 -exec sudo btrfs property set {} ro false \;

# delete any nested subvolumes (sudo because we might not have permissions to
# read the directories that they're found in)
sudo find . -depth -type d -inum 256 -exec btrfs subvolume delete {} \;
