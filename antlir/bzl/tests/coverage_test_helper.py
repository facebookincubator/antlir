#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


def coverage_test_helper() -> int:
    one = 10 + 3 - 12
    print("This function should be called {one} time to achieve 100% coverage")
    return one
