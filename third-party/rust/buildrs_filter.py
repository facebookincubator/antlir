#!/usr/bin/env python3
import sys
import re

cfg_re = re.compile(r"^cargo:rustc-cfg=(.*)$")

for line in sys.stdin:
    match = cfg_re.match(line)
    if match:
        print(f"--cfg\n{match.group(1)}")
