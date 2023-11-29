#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

ba="$1"
input="$2"
output="$3"

script="
"

script="
mount --make-rprivate /
mount --bind -o ro $ba $ba
exec chroot $ba /usr/bin/rpm2extents SHA256
"

exec unshare -mUr /bin/bash -ue -c "$script" < "$input" > "$output"
