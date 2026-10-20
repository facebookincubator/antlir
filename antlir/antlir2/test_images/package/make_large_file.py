#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Generate the deterministic "large file" used by standard_features packages.

The file exercises large-file / incompressible-content paths in package format
tests, so its bulk must be high-entropy. It used to be populated from
/dev/urandom, which made every build produce different bytes. Instead this
script emits SHAKE-128 in counter mode under a fixed seed: byte-identical on
every build, on every platform (SHAKE is a fixed standard, unlike e.g.
random.Random whose sequence is not guaranteed stable), and still
incompressible.

Layout (unchanged from the /dev/urandom version):
    LINE + 128MiB filler + LINE + 128MiB filler + LINE
for a total of 268435513 bytes.
"""

import hashlib
import sys

# Fixed seed for the SHAKE-128 counter-mode filler. Changing this seed changes
# every filler byte of the generated file.
SEED = b"antlir2-large-file-256M"
# Marker line interleaved with the filler: present 3x (start, middle, end).
LINE = b"antlir2-large-file\n"
CHUNK_SIZE = 1024 * 1024
# 2 halves x 128 chunks x 1MiB = 256MiB of filler.
HALF_CHUNKS = 128


def main() -> None:
    out_path = sys.argv[1]
    counter = 0
    with open(out_path, "wb") as f:
        f.write(LINE)
        for _ in range(2):
            for _ in range(HALF_CHUNKS):
                f.write(
                    hashlib.shake_128(SEED + counter.to_bytes(8, "little")).digest(
                        CHUNK_SIZE
                    )
                )
                counter += 1
            f.write(LINE)


if __name__ == "__main__":
    main()
