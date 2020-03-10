#!/bin/bash
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ue -o pipefail -o noclobber
my_path=$(readlink -f "$0")
my_dir=$(dirname "$my_path")
base_dir=$(dirname "$my_dir")
exec "$base_dir"/yum-dnf-from-snapshot \
    --repo-server "$base_dir/repo-server" \
    --snapshot-dir "$base_dir" \
    --storage "$(cat "$base_dir"/storage.json)" dnf "$@"
