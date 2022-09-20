#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import unittest
from contextlib import asynccontextmanager
from typing import Any, Dict

from antlir import update_package_db as updb
from antlir.fs_utils import Path, temp_dir


def _get_js_content(ss_hash: str, content: Dict[str, Any]) -> str:
    return f"""\
# {updb._GENERATED} SignedSource<<{ss_hash}>>
# Update via `how`
{json.dumps(content, sort_keys=True, indent=4)}
"""


def _write_json_db(json_path, dct) -> None:
    os.makedirs(json_path.dirname())
    with open(json_path, "w") as outfile:
        # Not using `_with_generated_header` to ensure that we are
        # resilient to changes in the header.
        outfile.write(f"# A {updb._GENERATED} file\n# second header line\n")
        json.dump(dct, outfile)


@asynccontextmanager
async def _base_get_db_info_fn(*args, **kwargs):
    async def _get_db_info(pkg, tag, opts):
        return pkg, tag, opts if opts else {"x": "z"}

    yield _get_db_info


# CLI has only minor parsing on top of the library function, so it's valuable to
# test both to ensure behaviour doesn't diverge. We thus use a base class to
# define most shared test cases.
class UpdatePackageDbTestBase:
    def _check_file(self, path, content) -> None:
        if not (os.path.exists(path)):
            db_dir = Path(path).dirname().dirname()
            # pyre-ignore[16]
            self.fail(
                f"File {path} did not exist, dir: {list(os.walk(db_dir))}"
            )
        with open(path) as infile:
            # pyre-ignore[16]
            self.assertEqual(content, infile.read())

    async def _update(
        self,
        db,
        pkg_updates=None,
        out_db=None,
        update_all: bool = True,
        get_db_info_fn=None,
        is_exception_skippable=None,
    ):
        raise NotImplementedError

    async def test_default_update(self) -> None:
        with temp_dir() as td:
            in_db = td / "idb"
            _write_json_db(in_db / "pkg" / "tag.json", {"foo": "bar"})
            _write_json_db(in_db / "pkg2" / "tag2.json", {"j": "k"})
            # pyre-fixme[16]: `UpdatePackageDbTestBase` has no attribute `assertEqual`.
            self.assertEqual(
                {"pkg": {"tag": {"foo": "bar"}}, "pkg2": {"tag2": {"j": "k"}}},
                updb._read_json_dir_db(in_db),
            )
            out_db = td / "odb"
            p1_out_path = out_db / "pkg" / "tag.json"
            p2_out_path = out_db / "pkg2" / "tag2.json"
            await self._update(db=in_db, out_db=out_db)
            # pyre-fixme[16]: `UpdatePackageDbTestBase` has no attribute
            #  `assertCountEqual`.
            self.assertCountEqual([b"pkg", b"pkg2"], (out_db).listdir())
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

    async def test_explicit_update(self) -> None:
        with temp_dir() as td:
            db_path = td / "idb"
            _write_json_db(
                db_path / "p1" / "tik.json", {"foo": "bar"}  # replaced
            )
            _write_json_db(db_path / "p2" / "tok.json", {"a": "b"})  # preserved
            await self._update(
                db=db_path,
                pkg_updates={
                    "p1": {
                        "tik": updb.PackageDbUpdate(
                            updb.UpdateAction.REPLACE, {"choo": "choo"}
                        )
                    },
                    "p2": {
                        "tak": updb.PackageDbUpdate(
                            updb.UpdateAction.CREATE,
                            {"boo": True},
                        )
                    },
                    "never": {
                        "seen": updb.PackageDbUpdate(
                            updb.UpdateAction.CREATE, {"oompa": "loompa"}
                        )
                    },
                },
                update_all=False,
            )
            self._check_file(
                db_path / "p1" / "tik.json",
                _get_js_content(
                    "b5c458dc21f07f3d4437a01c634876db",
                    {"choo": "choo"},
                ),
            )
            self._check_file(
                db_path / "p2" / "tok.json",
                _get_js_content("4897f4457e120a6852fc3873d70ad543", {"a": "b"}),
            )
            self._check_file(
                db_path / "p2" / "tak.json",
                _get_js_content(
                    "c6e75caa1da9ac63b0ef9151e783520f",
                    {"boo": True},
                ),
            )
            self._check_file(
                db_path / "never" / "seen.json",
                _get_js_content(
                    "6846ca9d4c83b71baa720d9791724ac0",
                    {"oompa": "loompa"},
                ),
            )

    async def test_explicit_update_conflicts(self) -> None:
        with temp_dir() as td:
            db_path = td / "idb"
            _write_json_db(db_path / "p1" / "a.json", {})
            _write_json_db(db_path / "p2" / "b.json", {})
            # pyre-fixme[16]: `UpdatePackageDbTestBase` has no attribute
            #  `assertRaisesRegex`.
            with self.assertRaisesRegex(
                updb.PackageExistsError, r".*create.*p1:a"
            ):
                await self._update(
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
                updb.PackageDoesNotExistError, r".*replace.*p2:c"
            ):
                await self._update(
                    db=db_path,
                    pkg_updates={
                        "p2": {
                            "c": updb.PackageDbUpdate(
                                updb.UpdateAction.REPLACE, {}
                            )
                        }
                    },
                )

    async def test_tag_deletion(self) -> None:
        @asynccontextmanager
        async def _none_get_db_info_fn(*args, **kwargs):
            async def _get_db_info_none(pkg, tag, opts):
                return pkg, tag, opts if opts else None

            yield _get_db_info_none

        with temp_dir() as td:
            db_path = td / "idb"
            _write_json_db(db_path / "p1" / "tik.json", {"a": "b"})
            _write_json_db(
                db_path / "p2" / "tok.json", {"y": "z"}
            )  # Will be deleted
            await self._update(
                db=db_path,
                pkg_updates={
                    "p1": {
                        "tik": updb.PackageDbUpdate(
                            updb.UpdateAction.REPLACE, {"c": "d"}
                        )
                    },
                    "never": {
                        "seen": updb.PackageDbUpdate(
                            updb.UpdateAction.CREATE, {"m": "n"}
                        )
                    },
                },
                # None will cause a deletion
                get_db_info_fn=_none_get_db_info_fn(),
            )
            self._check_file(
                db_path / "p1" / "tik.json",
                _get_js_content("1003a3786a74bb5fc2b817e752d3499c", {"c": "d"}),
            )
            self._check_file(
                db_path / "never" / "seen.json",
                _get_js_content("3b96485ebd8dad07ef3393861364407a", {"m": "n"}),
            )
            # Should have been deleted
            # pyre-fixme[16]: `UpdatePackageDbTestBase` has no attribute `assertFalse`.
            self.assertFalse((db_path / "p2" / "tok.json").exists())


class UpdatePackageDbCliTestCase(
    unittest.IsolatedAsyncioTestCase, UpdatePackageDbTestBase
):
    async def _update(
        self,
        db,
        pkg_updates=None,
        out_db=None,
        update_all: bool = True,
        get_db_info_fn=None,
        is_exception_skippable=None,
    ) -> None:
        args = [
            f"--db={db}",
            *([f"--out-db={out_db}"] if out_db else []),
            *(["--update-all"] if update_all else []),
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
        if get_db_info_fn is None:
            get_db_info_fn = _base_get_db_info_fn()
        await updb.main_cli(
            args,
            get_db_info_fn,
            how_to_generate="how",
            overview_doc="overview doc",
            options_doc="opts doc",
        )

    async def test_cli_option_conflicts(self) -> None:
        with temp_dir() as td:
            db_path = td / "idb"
            _write_json_db(db_path / "p1" / "a.json", {})
            _write_json_db(db_path / "p2" / "b.json", {})
            get_info_fn = _base_get_db_info_fn()
            with self.assertRaisesRegex(
                RuntimeError, r'Multiple updates.*p2:b.*"replace" with {}'
            ):
                await updb.main_cli(
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
                await updb.main_cli(
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

    async def test_cli_invalid_options_count(self) -> None:
        with temp_dir() as td:
            db_path = td / "idb"
            _write_json_db(db_path / "p1" / "a.json", {})
            get_info_fn = _base_get_db_info_fn()
            with self.assertRaisesRegex(
                RuntimeError, "Invalid options specified"
            ):
                await updb.main_cli(
                    [
                        f"--db={db_path}",
                        *("--create", "p1", "yo", "{}", "toomany"),
                    ],
                    get_info_fn,
                    how_to_generate="how",
                    overview_doc="overview doc",
                    options_doc="opts doc",
                )


class UpdatePackageDbLibraryTestCase(
    unittest.IsolatedAsyncioTestCase, UpdatePackageDbTestBase
):
    async def _update(
        self,
        db,
        pkg_updates=None,
        out_db=None,
        update_all: bool = True,
        get_db_info_fn=None,
        is_exception_skippable=None,
    ) -> None:
        if get_db_info_fn is None:
            get_db_info_fn = _base_get_db_info_fn()
        await updb.update_package_db(
            db_path=db,
            how_to_generate="how",
            get_db_info_factory=get_db_info_fn,
            out_db_path=out_db,
            update_all=update_all,
            pkg_updates=pkg_updates,
            is_exception_skippable=is_exception_skippable,
        )

    async def test_skippable_exception(self) -> None:
        @asynccontextmanager
        async def _exc_get_db_info_fn(*args, **kwargs):
            async def _get_db_info_exc(pkg, tag, opts):
                if (pkg, tag) == ("p2", "tok"):
                    raise AssertionError("it works!")
                return pkg, tag, opts if opts else {"xx": "zz"}

            yield _get_db_info_exc

        with temp_dir() as td:
            db_path = td / "idb"
            _write_json_db(db_path / "p1" / "tik.json", {"a1": "b1"})
            _write_json_db(db_path / "p2" / "tok.json", {"a2": "b2"})
            await self._update(
                db=db_path,
                get_db_info_fn=_exc_get_db_info_fn(),
                update_all=True,
                is_exception_skippable=lambda e: isinstance(e, AssertionError),
            )
            self._check_file(
                db_path / "p1" / "tik.json",
                _get_js_content(
                    "3e6962dd153ee611fd6b78163d3d9ccd", {"xx": "zz"}
                ),
            )
            # Unchanged due to skippable exception
            self._check_file(
                db_path / "p2" / "tok.json",
                _get_js_content(
                    "d677a771acf63e8058e979152dff8d06", {"a2": "b2"}
                ),
            )

            # Now ensure an unskippable exception propagates
            with self.assertRaisesRegex(AssertionError, "it works!"):
                await self._update(
                    db=db_path,
                    get_db_info_fn=_exc_get_db_info_fn(),
                    update_all=True,
                    is_exception_skippable=lambda e: isinstance(e, ValueError),
                )
