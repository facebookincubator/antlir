#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import unittest

from .. import update_package_db as updb
from ..common import nullcontext
from ..fs_utils import temp_dir


def _get_js_content(ss_hash: str, content: str):
    return f"""\
# {updb._GENERATED} SignedSource<<{ss_hash}>>
# Update via `how`
{json.dumps(content, sort_keys=True, indent=4)}
"""


class UpdatePackageDbTestCase(unittest.TestCase):
    def _check_file(self, path, content):
        with open(path) as infile:
            self.assertEqual(content, infile.read())

    def _write_json_db(self, json_path, dct):
        os.makedirs(json_path.dirname())
        with open(json_path, "w") as outfile:
            # Not using `_with_generated_header` to ensure that we are
            # resilient to changes in the header.
            outfile.write(f"# A {updb._GENERATED} file\n# second header line\n")
            json.dump(dct, outfile)

    def _main(self, argv):
        updb.main(
            argv,
            nullcontext(lambda _pkg, _tag, opts: opts if opts else {"x": "z"}),
            how_to_generate="how",
            overview_doc="overview doc",
            options_doc="opts doc",
        )

    def test_json_db(self):
        with temp_dir() as td:
            in_db = td / "idb"
            self._write_json_db(in_db / "pkg" / "tag.json", {"foo": "bar"})
            self.assertEqual(
                {"pkg": {"tag": {"foo": "bar"}}}, updb._read_json_dir_db(in_db)
            )
            out_db = td / "odb"
            out_path = out_db / "pkg" / "tag.json"
            self._main([f"--db={in_db}", f"--out-db={out_db}"])
            self.assertEqual([b"pkg"], (out_db).listdir())
            self.assertEqual([b"tag.json"], out_path.dirname().listdir())
            self._check_file(
                out_path,
                _get_js_content("e8b8ab0d998b5fe5429777af98579c12", {"x": "z"}),
            )

    def test_explicit_update(self):
        with temp_dir() as td:
            db_path = td / "idb"
            self._write_json_db(
                db_path / "p1" / "tik.json", {"foo": "bar"}  # replaced
            )
            self._write_json_db(
                db_path / "p2" / "tok.json", {"a": "b"}  # preserved
            )
            self._main(
                [
                    f"--db={db_path}",
                    *("--replace", "p1", "tik", '{"choo": "choo"}'),
                    *("--create", "p2", "tak", '{"boo": true}'),
                    *("--create", "never", "seen", '{"oompa": "loompa"}'),
                    "--no-update-existing",
                ]
            )
            self._check_file(
                db_path / "p1" / "tik.json",
                _get_js_content(
                    "b5c458dc21f07f3d4437a01c634876db", {"choo": "choo"}
                ),
            )
            self._check_file(
                db_path / "p2" / "tok.json",
                _get_js_content("4897f4457e120a6852fc3873d70ad543", {"a": "b"}),
            )
            self._check_file(
                db_path / "p2" / "tak.json",
                _get_js_content(
                    "c6e75caa1da9ac63b0ef9151e783520f", {"boo": True}
                ),
            )
            self._check_file(
                db_path / "never" / "seen.json",
                _get_js_content(
                    "6846ca9d4c83b71baa720d9791724ac0", {"oompa": "loompa"}
                ),
            )

    def test_explicit_update_conflicts(self):
        with temp_dir() as td:
            db_path = td / "idb"
            self._write_json_db(db_path / "p1" / "a.json", {})
            self._write_json_db(db_path / "p2" / "b.json", {})
            with self.assertRaisesRegex(AssertionError, "'p1', 'a'"):
                self._main([f"--db={db_path}", "--create", "p1", "a", "{}"])
            with self.assertRaisesRegex(AssertionError, "'p2', 'c'"):
                self._main([f"--db={db_path}", "--replace", "p2", "c", "{}"])
            with self.assertRaisesRegex(RuntimeError, 'Conflicting "replace"'):
                self._main(
                    [
                        f"--db={db_path}",
                        *("--replace", "p2", "b", "{}"),
                        *("--replace", "p2", "b", "{}"),
                    ]
                )
