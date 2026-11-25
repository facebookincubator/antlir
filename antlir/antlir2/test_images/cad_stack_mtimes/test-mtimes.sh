#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

# an input's modification time survives the parent layer's store and the
# child layer's rebuild of it
test "$(stat -c %Y /mtimes/from-input)" = 1000
# a file the build wrote, and the directory it wrote it in, record 0 rather
# than the time of the build, and a file installed from buck-out does not take
# the build host's time for it
test "$(stat -c %Y /mtimes/written)" = 0
test "$(stat -c %Y /mtimes/installed)" = 0
test "$(stat -c %Y /mtimes)" = 0
