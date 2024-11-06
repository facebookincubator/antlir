#!/bin/sh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

cmd=$1; shift
exec /usr/local/bin/buck2 --isolation-dir isolation_dir.$$ bxl \
    fbcode//antlir/bzl:test_suite.bxl:"$cmd" -- "$@"
