#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


class NspawnPlugin:
    wrap_setup_subvol = None
    wrap_setup = None
    wrap_post_setup_popen = None
