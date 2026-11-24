#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

found=0
for f in /var/lib/dpkg/info/*.conffiles; do
    pkg=$(basename "$f" .conffiles)
    while read -r flag path; do
        [ "$flag" = remove-on-upgrade ] || continue
        dpkg-query -W -f '${Conffiles}\n' "$pkg" | grep -qxF " $path newconffile remove-on-upgrade"
        found=1
    done < "$f"
done
test "$found" = 1
