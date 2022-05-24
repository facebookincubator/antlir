#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
from typing import Set

from antlir.fs_utils import Path


def get_rpms() -> Set[str]:
    return {
        rpm
        for rpm in subprocess.check_output(
            ["rpm", "-qa", "--queryformat", "%{NAME}\n"],
            text=True,
        ).split("\n")
        if rpm
    }


# pyre-fixme[2]: Parameter must be annotated.
def check_rpm_names(test_case, package, resource: str) -> None:
    with Path.resource(package, resource, exe=False) as expected_path, open(
        expected_path
    ) as expected_file:
        expected = {
            # `rpms-with-reason` adds a TAB-separated reason to the RPM name
            s.split("\t")[0].strip()
            for s in expected_file
        }
    test_case.assertEqual(
        expected,
        get_rpms(),
    )
