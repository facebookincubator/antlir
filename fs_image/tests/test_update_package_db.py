#!/usr/bin/env python3
import json
import os
import textwrap
import unittest

from fs_image.common import nullcontext
from fs_image.fs_utils import temp_dir
from fs_image import update_package_db as updb

_GENERATED = updb._GENERATED


class UpdatePackageDbTestCase(unittest.TestCase):

    def _check_file(self, path, content):
        with open(path) as infile:
            self.assertEqual(content, infile.read())

    def _write_bzl_db(self, db_path, dct):
        with open(db_path, 'w') as outfile:
            # Not using `_with_generated_header` to ensure that we are
            # resilient to changes in the header.
            outfile.write(f'# A {_GENERATED} file\n# second header line\n')
            outfile.write(updb._BZL_DB_PREFIX)
            json.dump(dct, outfile)
        # Make sure our write implementation is sane.
        self.assertEqual(dct, updb._read_bzl_db(db_path))

    def _main(self, argv):
        updb.main(
            argv,
            nullcontext(lambda _pkg, _tag, opts: opts if opts else {'x': 'z'}),
            how_to_generate='how',
            overview_doc='overview doc',
            options_doc='opts doc',
        )

    def test_default_update(self):
        with temp_dir() as td:
            db_path = td / 'db.bzl'
            self._write_bzl_db(db_path, {'pkg': {'tag': {'foo': 'bar'}}})
            self._main(['--db', db_path.decode()])
            self._check_file(db_path, '# ' + _GENERATED + textwrap.dedent(''' \
            SignedSource<<69d45bae7b77e0bd2ee0d5a285d6fdb3>>
            # Update via `how`
            package_db = {
                "pkg": {
                    "tag": {
                        "x": "z",
                    },
                },
            }
            '''))

    def test_explicit_update(self):
        with temp_dir() as td:
            db_path = td / 'db.bzl'
            self._write_bzl_db(db_path, {
                'p1': {'tik': {'foo': 'bar'}},  # replaced
                'p2': {'tok': {'a': 'b'}},  # preserved
            })
            self._main([
                '--db', db_path.decode(),
                '--replace', 'p1', 'tik', '{"choo": "choo"}',
                '--create', 'p2', 'tak', '{"boo": true}',
                '--create', 'never', 'seen', '{"oompa": "loompa"}',
                '--no-update-existing',
            ])
            self._check_file(db_path, '# ' + _GENERATED + textwrap.dedent(''' \
            SignedSource<<1b43eea483a42dd704883a7021e259e0>>
            # Update via `how`
            package_db = {
                "never": {
                    "seen": {
                        "oompa": "loompa",
                    },
                },
                "p1": {
                    "tik": {
                        "choo": "choo",
                    },
                },
                "p2": {
                    "tak": {
                        "boo": True,
                    },
                    "tok": {
                        "a": "b",
                    },
                },
            }
            '''))

    def test_explicit_update_conflicts(self):
        with temp_dir() as td:
            db_path = td / 'db.bzl'
            self._write_bzl_db(db_path, {'p1': {'a': {}}, 'p2': {'b': {}}})
            with self.assertRaisesRegex(AssertionError, "'p1', 'a'"):
                self._main([
                    '--db', db_path.decode(), '--create', 'p1', 'a', '{}',
                ])
            with self.assertRaisesRegex(AssertionError, "'p2', 'c'"):
                self._main([
                    '--db', db_path.decode(), '--replace', 'p2', 'c', '{}',
                ])
            with self.assertRaisesRegex(RuntimeError, 'Conflicting "replace"'):
                self._main([
                    '--db', db_path.decode(),
                    '--replace', 'p2', 'b', '{}',
                    '--replace', 'p2', 'b', '{}',
                ])

    def test_json_db(self):
        with temp_dir() as td:
            os.makedirs(td / 'idb/pkg')
            with open(td / 'idb/pkg/tag.json', 'w') as outfile:
                # Not using `_with_generated_header` to ensure that we are
                # resilient to changes in the header.
                outfile.write(f'# A {_GENERATED} file\n# 2nd header line\n')
                json.dump({'foo': 'bar'}, outfile)
            self.assertEqual(
                {'pkg': {'tag': {'foo': 'bar'}}},
                updb._read_json_dir_db(td / 'idb'),
            )
            self._main([
                '--db', (td / 'idb').decode(),
                '--out-db', (td / 'odb').decode(),
            ])
            self.assertEqual([b'pkg'], os.listdir(td / 'odb'))
            self.assertEqual([b'tag.json'], os.listdir(td / 'odb/pkg'))
            self._check_file(
                td / 'odb/pkg/tag.json',
                '# ' + _GENERATED + textwrap.dedent(''' \
                SignedSource<<e8b8ab0d998b5fe5429777af98579c12>>
                # Update via `how`
                {
                    "x": "z"
                }
                '''))
