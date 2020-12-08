#!/usr/bin/python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
import json
import unittest

from antlir.serialize_targets_and_outputs import parse_and_dump


class TestSerializeTargetsAndOutputs(unittest.TestCase):
    def _run_test(self, targets_and_locs, delim):
        input_data = io.StringIO(
            delim.join(
                [
                    tl
                    for elem in zip(
                        targets_and_locs.keys(), targets_and_locs.values()
                    )
                    for tl in elem
                ]
            )
        )

        output = io.StringIO()
        expected_output = json.dumps(targets_and_locs)

        parse_and_dump(stdin=input_data, stdout=output, delim=delim)

        self.assertEqual(output.getvalue(), expected_output)

    def test_simple_case(self):
        self._run_test(
            targets_and_locs={
                "//this/is/a:target": "/this/is/the/target/location"
            },
            delim="|",
        )

    def test_unicode_case(self):
        self._run_test(
            targets_and_locs={"//this/is/crap:💩": "/this/is/crap/💩"},
            delim="☃",
        )

    def test_space_case(self):
        self._run_test(
            targets_and_locs={
                "//this/has a/space:in it": "/this/has a/space/in it"
            },
            delim="|",
        )
