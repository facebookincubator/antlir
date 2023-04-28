#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import subprocess
import sys

expected = json.loads(sys.argv[1])

for spec in expected["installed"]:
    if (
        subprocess.run(
            ["rpm", "-q", spec], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        ).returncode
        != 0
    ):
        print(f"'{spec}' is not installed")
        print()
        print("all installed rpms:")
        all_installed_rpms = subprocess.run(
            ["rpm", "-qa"], capture_output=True, text=True, check=True
        ).stdout.splitlines()
        all_installed_rpms.sort()
        for rpm in all_installed_rpms:
            print(rpm)
        sys.exit(1)

for spec in expected["userinstalled"]:
    installed_spec = subprocess.run(
        ["rpm", "-q", spec], capture_output=True, text=True, check=True
    ).stdout.strip()
    userinstalled_spec = subprocess.run(
        ["dnf", "repoquery", "--userinstalled", spec],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    if not userinstalled_spec or userinstalled_spec == installed_spec:
        print(f"'{spec}' is installed ({installed_spec}) but is not userinstalled")
        sys.exit(1)

for spec in expected["installed_not_userinstalled"]:
    installed_spec = subprocess.run(
        ["rpm", "-q", spec], capture_output=True, text=True, check=True
    ).stdout.strip()
    userinstalled_spec = subprocess.run(
        ["dnf", "repoquery", "--userinstalled", spec],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    if userinstalled_spec:
        print(f"'{spec}' is installed and userinstalled ({userinstalled_spec})")
        sys.exit(1)

for spec in expected["not_installed"]:
    proc = subprocess.run(["rpm", "-q", spec], capture_output=True, text=True)
    if proc.returncode == 0:
        print(f"'{spec}' is installed")
        print(proc.stdout)
        sys.exit(1)
    else:
        # rpm can fail for a number of reasons, so let's make sure that the
        # output looks like the requested rpm is installed
        if spec not in proc.stdout:
            print(f"unknown rpm failure: {proc.stdout}\n{proc.stderr}")
            sys.exit(2)
