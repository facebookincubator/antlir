#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -euo pipefail
root="$1"
shift

# Packages in trixie use merged-/usr paths. Preserve these aliases while
# extracting packages that still contain legacy directory entries.
mkdir -p "$root"/usr/{bin,sbin,lib,lib64}
for dir in bin sbin lib lib64; do
    ln -s "usr/$dir" "$root/$dir"
done
for deb in "$@"; do
    member=$(ar t "$deb" | sed -n '/^data\.tar/p')
    case "$member" in
        data.tar.xz) decompress=(xz -d) ;;
        data.tar.gz) decompress=(gzip -d) ;;
        data.tar.zst) decompress=(zstd -d) ;;
        data.tar) decompress=(cat) ;;
        *) echo "Unsupported deb payload: $member" >&2; exit 1 ;;
    esac
    ar p "$deb" "$member" | "${decompress[@]}" | tar --no-same-owner --keep-directory-symlink -x -C "$root"
done

# The appliance only runs tools; installing its packages with apt would
# require an appliance to resolve apt first. Set up the few files normally
# created by maintainer scripts that those tools need.
cp "$root/usr/share/base-passwd/passwd.master" "$root/etc/passwd"
cp "$root/usr/share/base-passwd/group.master" "$root/etc/group"
mkdir -p "$root"/{dev,proc,sys,tmp,var/log} \
    "$root/__antlir2__"/{root,build_appliance,working_directory,out}
