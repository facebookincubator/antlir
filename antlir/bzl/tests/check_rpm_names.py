#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess

from antlir.fs_utils import Path


def check_rpm_names(test_case, package, resource):
    with Path.resource(package, resource, exe=False) as expected_path, open(
        expected_path
    ) as expected_file:
        expected = {s.strip("\n") for s in expected_file}
    test_case.assertEqual(
        expected,
        {
            rpm
            for rpm in subprocess.check_output(
                ["rpm", "-qa", "--queryformat", "%{NAME}\n"],
                text=True,
            ).split("\n")
            if rpm
        },
    )
