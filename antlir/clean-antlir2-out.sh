#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

if [ ! -d "antlir2-out" ]; then
    cd "$(hg root)"
fi

declare -a working_volumes=()
declare -A seen_working_volumes=()

if [ -d "antlir2-out" ]; then
    working_volumes+=("antlir2-out")
fi

if [ -e ".eden/root" ] && command -v mkscratch >/dev/null 2>&1; then
    eden_root="$(readlink ".eden/root" || true)"
    if [ -n "${eden_root}" ]; then
        scratch_working_volume="$(
            mkscratch path "${eden_root}" --subdir antlir2-out 2>/dev/null || true
        )"
        if [ -n "${scratch_working_volume}" ] && [ -d "${scratch_working_volume}" ]; then
            working_volumes+=("${scratch_working_volume}")
        fi
    fi
fi

if [ "${#working_volumes[@]}" -eq 0 ]; then
    echo "no antlir2-out found in repo root or mkscratch path, exiting..."
    exit
fi

for working_volume in "${working_volumes[@]}"; do
    resolved_working_volume="$(
        readlink -f "${working_volume}" 2>/dev/null || echo "${working_volume}"
    )"
    if [ -n "${seen_working_volumes[${resolved_working_volume}]+x}" ]; then
        continue
    fi
    seen_working_volumes["${resolved_working_volume}"]=1

    echo "cleaning antlir2 subvolumes in ${working_volume}"
    pushd "${working_volume}"

    # Quick pass to just delete all the top-level subvolumes. Some of these
    # might fail if there are nested subvols but that's ok
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
    popd
done
