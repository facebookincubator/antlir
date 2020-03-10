#!/bin/bash -uex
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -o pipefail
test $# -eq 3
tarball_name=$1  # Exercises `generator_args`
prebuilt_tarball=$2  # Compensates for non-stable hashes of generated tar
out_dir=$3  # Provided per the tarball generator contract

cp -a "$prebuilt_tarball" "$out_dir/$tarball_name"

echo "$tarball_name"  # Required by the tarball generator contract
