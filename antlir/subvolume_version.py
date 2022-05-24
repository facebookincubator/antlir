#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import base64
import os
import random
import sys
import time


# pyre-fixme[2]: Parameter must be annotated.
def b64(i) -> bytes:
    return base64.urlsafe_b64encode(i.to_bytes(16, "big").lstrip(b"\0")).strip(
        b"="
    )


# '.' is not part of the `urlsafe_b64encode` alphabet.
sys.stdout.buffer.write(
    # At 10ms resolution, this will be 7 bytes for the next 100 years.
    b64(int(time.time() * 100))
    + b"."
    # It's VERY unlikely (or impossible, depending on `pid_max`) for a
    # modern Linux to cycle its PIDs within 10ms.
    + b64(os.getpid())
    + b"."
    # For good measure, add 4 B64 bytes of randomness.
    + b64(random.randrange(2**24))
)
