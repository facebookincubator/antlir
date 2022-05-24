#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys

# pyre-fixme[5]: Global expression must be annotated.
kind = sys.argv[1]
if kind == "only_write_to_stdout":  # see `test_boot_marked_as_non_build_step`
    print("fake_service:", kind)
else:
    open(f"/fake-{kind}-service-ran", "w").close()
