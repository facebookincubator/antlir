#!/bin/bash -uex
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

#
# `tests/test_subvolume_garbage_collector.py` uses this fake in order to
# avoid instantiating actual btrfs subvolumes to delete.

# Lets us test that the sudo override is working instead of failing later.
if [[ "$*" == "MAGIC_SENTINEL" ]] ; then
  echo -n "$*"
  exit 0
fi

die() {
  echo "$@" 1>&2
  exit 1
}

[[ "$#" == "4" ]] || die "Bad arg count:" "$@"
[[ "$1" == "btrfs" ]] || die "Bad arg 1: $1"
[[ "$2" == "subvolume" ]] || die "Bad arg 2: $2"
[[ "$3" == "delete" ]] || die "Bad arg 3: $3"

rmdir "$4"
