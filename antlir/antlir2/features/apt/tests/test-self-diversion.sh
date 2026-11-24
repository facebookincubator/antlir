#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

test "$(cat /antlir2-diverted)" = "diverter"
test ! -e /antlir2-diverted.distrib
test "$(dpkg-divert --truename /antlir2-diverted)" = "/antlir2-diverted.distrib"

# libc-bin (Essential) was configured before antlir2-after-essential was
# unpacked
test "$(cat /antlir2-libc-bin-status)" = "configured"
