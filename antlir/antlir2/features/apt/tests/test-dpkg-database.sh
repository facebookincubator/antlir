#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

# every listed path exists, so names with escapes in dpkg-deb --contents
# (system-systemd\x2dcryptsetup.slice) are recorded as they are
dpkg-query -L systemd | while read -r path; do
    test -e "$path" || test -L "$path"
done

conffiles=$(dpkg-query -W -f '${Conffiles}\n' systemd)
test -n "$conffiles"
echo "$conffiles" | while read -r path md5; do
    [ -n "$path" ] || continue
    test "$(md5sum < "$path" | cut -d' ' -f1)" = "$md5"
done

# the paths packages are interested in are recorded as dpkg records them
cat /var/lib/dpkg/triggers/File
grep -qx '/usr/lib/systemd/catalog systemd/noawait' /var/lib/dpkg/triggers/File

# a new info database is in format 1, so a Multi-Arch: same package's info
# files carry its architecture
test "$(cat /var/lib/dpkg/info/format)" = 1
test -f "/var/lib/dpkg/info/libc6:$(dpkg --print-architecture).list"
dpkg -L libc6 | grep -q '/libc\.so\.6$'
