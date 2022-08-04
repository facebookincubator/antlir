#!/usr/bin/python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
import json
import unittest
from contextlib import contextmanager
from typing import Generator, Mapping

from antlir.artifacts_dir import SigilNotFound

from antlir.fs_utils import Path, temp_dir
from antlir.serialize_targets_and_outputs import (
    BuckConfigParser,
    main as serialize_targets_and_outputs,
)


class TestSerializeTargetsAndOutputs(unittest.TestCase):
    @contextmanager
    def _cell_root(self, main_cell: str = "") -> Generator[Path, None, None]:
        assert main_cell not in {
            "antlir",
            "config",
        }, "These cell names are reserved"
        buck_config = BuckConfigParser()
        buck_config["repositories"] = {
            "antlir": "../antlir",
            "config": "config",
            **({main_cell: "."} if main_cell else {}),
        }
        with temp_dir() as cell_root:
            with open(cell_root / ".buckconfig", "w") as config_file:
                buck_config.write(config_file)
            yield cell_root

    def _expected_output(
        self,
        targets_and_locs: Mapping[str, Path],
        main_cell: str,
    ) -> Mapping[str, str]:
        ret = {}
        for target, loc in targets_and_locs.items():
            ret[target] = str(loc)
            if target.startswith("//"):
                ret[main_cell + target] = str(loc)
            if target.startswith(main_cell + "//"):
                ret[target[len(main_cell) :]] = str(loc)
        return ret

    def _run_test(self, targets_and_locs, delim, main_cell: str = "") -> None:
        with self._cell_root(main_cell) as cell_root:
            targets_and_locs = {
                target: cell_root / "buck-out" / loc
                for target, loc in targets_and_locs.items()
            }
            input_data = io.StringIO(
                delim.join(
                    [
                        str(tl)
                        for elem in zip(
                            targets_and_locs.keys(), targets_and_locs.values()
                        )
                        for tl in elem
                    ]
                )
            )

            output = io.StringIO()
            serialize_targets_and_outputs(
                stdin=input_data,
                stdout=output,
                delim=delim,
            )

        self.assertEqual(
            json.loads(output.getvalue()),
            self._expected_output(targets_and_locs, main_cell),
        )

    def test_simple_case(self) -> None:
        self._run_test(
            targets_and_locs={
                "//this/is/a:target": "this/is/the/target/location"
            },
            delim="|",
        )

    def test_unicode_case(self) -> None:
        self._run_test(
            targets_and_locs={"//this/is/crap:💩": "this/is/crap/💩"},
            delim="☃",
        )

    def test_space_case(self) -> None:
        self._run_test(
            targets_and_locs={
                "//this/has a/space:in it": "this/has a/space/in it"
            },
            delim="|",
        )

    def test_multi_cell(self) -> None:
        self._run_test(
            targets_and_locs={
                "//foo:bar": "foo/bar",
                "A//baz:qux": "baz/qux",
                "B//foo:bar": "foo/bar",
            },
            delim="|",
            main_cell="A",
        )

    def test_multi_cell_no_main_cell_name(self) -> None:
        self._run_test(
            targets_and_locs={
                "//foo:bar": "foo/bar",
                "B//foo:bar": "foo/bar",
            },
            delim="|",
            main_cell="",
        )

    def test_cannot_find_cell_root(self) -> None:
        self.assertRaises(
            # pyre-fixme[16]: Module `artifacts_dir_rs` has no attribute
            #  `SigilNotFound`.
            SigilNotFound,
            self._run_test,
            targets_and_locs={
                "//foo:bar": "/foo/bar",
            },
            delim="|",
        )
