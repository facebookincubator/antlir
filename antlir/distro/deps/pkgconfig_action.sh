#!/usr/bin/env bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -euo pipefail

lib="$1"; shift
sysroot="$1"; shift
cflags_out="$1"; shift
libs_out="$1"; shift

PKG_CONFIG_SYSROOT_DIR="$sysroot" PKG_CONFIG_PATH="$sysroot/usr/lib64/pkgconfig" \
    pkg-config "$lib" --cflags > "$cflags_out"

PKG_CONFIG_SYSROOT_DIR="$sysroot" PKG_CONFIG_PATH="$sysroot/usr/lib64/pkgconfig" \
    pkg-config "$lib" --libs > "$libs_out"
