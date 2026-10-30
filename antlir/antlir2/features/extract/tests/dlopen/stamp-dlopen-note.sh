#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -e

json='[{"feature":"acl","description":"Support for file Access Control Lists","priority":"recommended","soname":["libacl.so.1"]},{"feature":"blkid","description":"Support for Block Device Identification","priority":"recommended","soname":["libblkid.so.1"]},{"feature":"seccomp","description":"Support for Seccomp Sandboxes","priority":"recommended","soname":["libseccomp.so.2"]},{"feature":"selinux","description":"Support for SELinux","priority":"recommended","soname":["libselinux.so.1"]},{"feature":"cap","description":"Support for capabilities","priority":"recommended","soname":["libcap.so.2"]}]'
note=/tmp/dlopen-note.bin
descsz=$(( ${#json} + 1 ))
pad=$(( (4 - descsz % 4) % 4 ))

{
  printf '\x04\x00\x00\x00'
  printf '%b' "$(printf '\\x%02x\\x%02x\\x00\\x00' $(( descsz & 255 )) $(( descsz >> 8 )))"
  printf '\x0a\x0c\x7c\x40'
  printf 'FDO\0'
  printf '%s\0' "$json"
  head -c "$pad" /dev/zero
} > "$note"

for lib in /usr/lib64/systemd/libsystemd-shared-*.so; do
  objcopy \
    --add-section .note.dlopen="$note" \
    --set-section-flags .note.dlopen=alloc,readonly \
    "$lib"
done
rm "$note"
