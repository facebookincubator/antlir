#!/bin/bash -ue
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -o pipefail
#
# Executed under `sudo` by `get_volume_for_current_repo()`. Makes sure that
# the given btrfs volume path is tagged with the absolute path of the source
# repo, and is not used by any other source repo.
#
# CAREFUL: This operation is not atomic, so if there is any chance it might
# run concurrently, be sure to wrap this script with `flock`.
#

min_bytes="${1:?argument 1 resizes the volume to have this many free bytes}"
image="${2:?argument 2 must be a path to a btrfs image, which may get erased}"
volume="${3:?argument 3 must be the path for the btrfs volume mount}"

assert() {
  (eval "$@") || (
    echo "Assertion failed:" "$@" 1>&2
    exit 1
  )
}

ensure_permissions() {
  # Explicitly set the image to read/write only by root to prevent potential
  # leaking of sensitive information to unprivileged users.
  chown root:root "$image"
  chmod 0600 "$image"
}

mount_image() {
  # Silently patch permissions of existing images.
  ensure_permissions
  echo "Mounting btrfs $image at $volume"
  # Explicitly set filesystem type to detect shenanigans.
  mount -t btrfs -o loop,discard,nobarrier "$image" "$volume"
  # Mark our mount "private".  We do not accept propagation events from the
  # parent mount, and will not send events outside of "$volume".  And any
  # mounts made within the volume should be contained to the volume.
  # But really, there just shouldn't be any mounts made within this
  # volume that are *not* inside a container.
  mount --make-private "$volume"
}

resize_image() {
  # Future: maybe we shouldn't hardcode 4096, but instead query:
  #   blockdev --getbsz $loop_dev
  local block_sz=4096
  local bytes="$1"
  local rounded_bytes
  # Avoid T24578982: btrfs soft lockup: `losetup --set-capacity /dev/loopN`
  # wrongly sets block size to 1024 when backing file size is 4096-odd.
  rounded_bytes=$(( bytes + ((block_sz - (bytes % block_sz)) % block_sz) ))
  if [[ "$bytes" != "$rounded_bytes" ]] ; then
    echo "Rounded image size up to $rounded_bytes to work around kernel bug."
  fi
  truncate -s "$rounded_bytes" "$image"
}

format_image() {
  echo "Formatting empty btrfs of $min_bytes bytes at $image"
  local min_usable_fs_size=$((175 * 1024 * 1024))
  if [[ "$min_bytes" -lt "$min_usable_fs_size" ]] ; then
    # Would get:
    #  < 100MB: ERROR: not enough free space to allocate chunk
    #  < 175MB: ERROR: unable to resize '_foo/volume': Invalid argument
    echo "btrfs filesystems of < $min_usable_fs_size do not work well, growing"
    min_bytes="$min_usable_fs_size"
  fi
  resize_image "$min_bytes"
  mkfs.btrfs "$image"
  ensure_permissions
}

ensure_mounted() {
  mkdir -p "$volume"
  # Don't bother checking if $volume is another kind of mount, since we will
  # just proceed to mount over it.
  if [[ "$(findmnt --noheadings --output FSTYPE "$volume")" != btrfs ]] ; then
    # Do a checksum scrub -- since we run with nobarrier and --direct-io, it
    # is entirely possible that a power failure will corrupt the image.
    btrfs check --check-data-csum "$image" || format_image
    # If it looks like we have a valid image, just re-use it. This allows us
    # to recover built images after a restart.
    mount_image || (format_image && mount_image)
  fi
  local loop_dev
  loop_dev=$(findmnt --noheadings --output SOURCE "$volume")
  # This helps perf and avoids doubling our usage of buffer cache.
  losetup --direct-io=on "$loop_dev" ||
    echo "Could not enable --direct-io for $loop_dev, expect worse performance"

  local free_bytes
  # Future: Consider using `btrfs filesystem usage -b "$volume" | grep "min:"`
  free_bytes=$(findmnt --bytes --noheadings --output AVAIL "$volume")
  local growth_bytes
  growth_bytes=$((min_bytes - free_bytes))

  if [[ "$growth_bytes" -gt 0 ]] ; then
    echo "Growing $image by $growth_bytes bytes"
    local old_bytes
    old_bytes=$(stat --format=%s "$image")
    local new_bytes
    new_bytes=$((old_bytes + growth_bytes))
    # Paranoid assertions in case of integer overflow or similar bugs
    assert [[ "$new_bytes" -gt "$old_bytes" ]]
    assert [[ $((new_bytes - growth_bytes)) -eq "$old_bytes" ]]
    resize_image "$new_bytes"
    losetup --set-capacity "$loop_dev"
    btrfs filesystem resize max "$volume"
  fi
}

ensure_mounted 1>&2  # In Buck, stderr is more useful
