#!/usr/bin/env python3

# Extract additional rustc flags to pass as part of rust_library rules from the
# output of build.rs, as normally parsed by cargo
import re
import sys

cfg_re = re.compile(r"^cargo:rustc-cfg=(.*)$")

for line in sys.stdin:
    match = cfg_re.match(line)
    if match:
        print(f"--cfg\n{match.group(1)}")
