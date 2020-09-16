#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import unittest
from contextlib import nullcontext

from .. import update_package_db as updb
from ..fs_utils import temp_dir


def _get_js_content(ss_hash: str, content: str):
    return f"""\
# {updb._GENERATED} SignedSource<<{ss_hash}>>
# Update via `how`
{json.dumps(content, sort_keys=True, indent=4)}
"""


def _write_json_db(json_path, dct):
    os.makedirs(json_path.dirname())
    with open(json_path, "w") as outfile:
        # Not using `_with_generated_header` to ensure that we are
        # resilient to changes in the header.
        outfile.write(f"# A {updb._GENERATED} file\n# second header line\n")
        json.dump(dct, outfile)


# CLI has only minor parsing on top of the library function, so it's valuable to
# test both to ensure behaviour doesn't diverge. We thus use a base class to
# define most shared test cases.
class UpdatePackageDbTestBase:
    def _check_file(self, path, content):
        with open(path) as infile:
            self.assertEqual(content, infile.read())

    def _update(
        self, db, pkg_updates=None, out_db=None, no_update_existing=False
    ):
        raise NotImplementedError

    def test_default_update(self):
        with temp_dir() as td:
            in_db = td / "idb"
            _write_json_db(in_db / "pkg" / "tag.json", {"foo": "bar"})
            _write_json_db(in_db / "pkg2" / "tag2.json", {"j": "k"})
            self.assertEqual(
                {"pkg": {"tag": {"foo": "bar"}}, "pkg2": {"tag2": {"j": "k"}}},
                updb._read_json_dir_db(in_db),
            )
            out_db = td / "odb"
            p1_out_path = out_db / "pkg" / "tag.json"
            p2_out_path = out_db / "pkg2" / "tag2.json"
            self._update(db=in_db, out_db=out_db)
            self.assertEqual([b"pkg", b"pkg2"], (out_db).listdir())
            self.assertEqual([b"tag.json"], p1_out_path.dirname().listdir())
            self.assertEqual([b"tag2.json"], p2_out_path.dirname().listdir())
            # Note the x/z dict is returned for `opts` in `_main` above
            self._check_file(
                p1_out_path,
                _get_js_content("e8b8ab0d998b5fe5429777af98579c12", {"x": "z"}),
            )
            self._check_file(
                p2_out_path,
                _get_js_content("e8b8ab0d998b5fe5429777af98579c12", {"x": "z"}),
            )

    def test_explicit_update(self):
        with temp_dir() as td:
            db_path = td / "idb"
            _write_json_db(
                db_path / "p1" / "tik.json", {"foo": "bar"}  # replaced
            )
            _write_json_db(db_path / "p2" / "tok.json", {"a": "b"})  # preserved
            self._update(
                db=db_path,
                pkg_updates={
                    "p1": {
                        "tik": updb.PackageDbUpdate(
                            updb.UpdateAction.REPLACE, {"choo": "choo"}
                        )
                    },
                    "p2": {
                        "tak": updb.PackageDbUpdate(
                            updb.UpdateAction.CREATE, {"boo": True}
                        )
                    },
                    "never": {
                        "seen": updb.PackageDbUpdate(
                            updb.UpdateAction.CREATE, {"oompa": "loompa"}
                        )
                    },
                },
                no_update_existing=True,
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
            _write_json_db(db_path / "p1" / "a.json", {})
            _write_json_db(db_path / "p2" / "b.json", {})
            with self.assertRaisesRegex(
                AssertionError, r"Attempting.*create.*p1:a"
            ):
                self._update(
                    db=db_path,
                    pkg_updates={
                        "p1": {
                            "a": updb.PackageDbUpdate(
                                updb.UpdateAction.CREATE, {}
                            )
                        }
                    },
                )
            with self.assertRaisesRegex(
                AssertionError, r"Attempting.*replace.*p2:c"
            ):
                self._update(
                    db=db_path,
                    pkg_updates={
                        "p2": {
                            "c": updb.PackageDbUpdate(
                                updb.UpdateAction.REPLACE, {}
                            )
                        }
                    },
                )


class UpdatePackageDbCliTestCase(UpdatePackageDbTestBase, unittest.TestCase):
    def _update(
        self, db, pkg_updates=None, out_db=None, no_update_existing=False
    ):
        args = [
            f"--db={db}",
            *([f"--out-db={out_db}"] if out_db else []),
            *(["--no-update-existing"] if no_update_existing else []),
        ]
        for pkg, tag_to_update in (pkg_updates or {}).items():
            for tag, update in tag_to_update.items():
                args.extend(
                    [
                        f"--{update.action.value}",
                        pkg,
                        tag,
                        json.dumps(update.options),
                    ]
                )

        updb.main_cli(
            args,
            nullcontext(lambda _pkg, _tag, opts: opts if opts else {"x": "z"}),
            how_to_generate="how",
            overview_doc="overview doc",
            options_doc="opts doc",
        )

    def test_cli_option_conflicts(self):
        with temp_dir() as td:
            db_path = td / "idb"
            _write_json_db(db_path / "p1" / "a.json", {})
            _write_json_db(db_path / "p2" / "b.json", {})
            get_info_fn = nullcontext(
                lambda _pkg, _tag, opts: opts if opts else {"x": "z"}
            )
            with self.assertRaisesRegex(
                RuntimeError, r'Multiple updates.*p2:b.*"replace" with {}'
            ):
                updb.main_cli(
                    [
                        f"--db={db_path}",
                        *("--replace", "p2", "b", "{}"),
                        *("--replace", "p2", "b", "{}"),
                    ],
                    get_info_fn,
                    how_to_generate="how",
                    overview_doc="overview doc",
                    options_doc="opts doc",
                )
            with self.assertRaisesRegex(
                RuntimeError,
                # Don't rely on ordering of options in error message
                r"(\{'c': 'd'\}.*\{'a': 'b'\})"
                r"|(\{'a': 'b'\} .*\{'c': 'd'\})",
            ):
                updb.main_cli(
                    [
                        f"--db={db_path}",
                        *("--replace", "p2", "b", '{"a": "b"}'),
                        *("--create", "p2", "b", '{"c": "d"}'),
                    ],
                    get_info_fn,
                    how_to_generate="how",
                    overview_doc="overview doc",
                    options_doc="opts doc",
                )


class UpdatePackageDbLibraryTestCase(
    UpdatePackageDbTestBase, unittest.TestCase
):
    def _update(
        self, db, pkg_updates=None, out_db=None, no_update_existing=False
    ):
        updb.update_package_db(
            db_path=db,
            how_to_generate="how",
            get_db_info_factory=nullcontext(
                lambda _pkg, _tag, opts: opts if opts else {"x": "z"}
            ),
            out_db_path=out_db,
            update_existing=not no_update_existing,
            pkg_updates=pkg_updates,
        )
