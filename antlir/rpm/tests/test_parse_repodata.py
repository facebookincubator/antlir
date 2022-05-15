#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import bz2
import gzip
import lzma
import os
import unittest
from io import BytesIO
from typing import Iterator, Set, Tuple

from antlir.fs_utils import Path

from ..parse_repodata import get_rpm_parser, pick_primary_repodata
from ..repo_objects import Repodata, RepoMetadata
from ..tests.temp_repos import (
    get_test_signing_key,
    SAMPLE_STEPS,
    temp_repos_steps,
)


def _dir_paths(path: Path) -> Set[Path]:
    return {path / p for p in path.listdir()}


def find_test_repos(repos_root: Path) -> Iterator[Tuple[Path, RepoMetadata]]:
    for step_path in _dir_paths(repos_root):
        for p in _dir_paths(step_path):
            if p.basename() in [b"yum.conf", b"dnf.conf"]:
                continue
            with open(p / "repodata/repomd.xml", "rb") as f:
                yield p, RepoMetadata.new(xml=f.read())


def _rpm_set(infile: BytesIO, rd: Repodata):
    rpms = set()
    with get_rpm_parser(rd) as parser:
        while True:  # Exercise feed-in-chunks behavior
            chunk = infile.read(127)  # Our repodatas are tiny
            if not chunk:
                break
            rpms.update(parser.feed(chunk))
    assert len(rpms) > 0  # we have no empty test repos
    return rpms


class ParseRepodataTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Since we only read the repo, it is much faster to create
        # it once for all the tests (~2x speed-up as of writing).
        #
        # NB: This uses the fairly large "SAMPLE_STEPS" (instead of a more
        # minimal `repo_change_steps` used in most other tests) because it
        # **might** improve the tests' power.  This is NOT needed for code
        # coverage, so if you have a perf concern about this test, it is
        # fine to reduce the scope.
        cls.temp_repos_ctx = temp_repos_steps(
            gpg_signing_key=get_test_signing_key(),
            repo_change_steps=SAMPLE_STEPS,
        )
        cls.repos_root = cls.temp_repos_ctx.__enter__()

    @classmethod
    def tearDownClass(cls):
        cls.temp_repos_ctx.__exit__(None, None, None)

    def _xml_and_sqlite_primaries(
        self, repomd: RepoMetadata
    ) -> Tuple[Repodata, Repodata]:
        primaries = [
            (rd.is_primary_sqlite(), rd.is_primary_xml(), rd)
            for rd in repomd.repodatas
            if rd.is_primary_sqlite() or rd.is_primary_xml()
        ]
        primaries.sort()
        # All our test repos have both SQLite and XML generated.
        self.assertEqual(
            [(False, True), (True, False)],
            [(sql, xml) for sql, xml, _ in primaries],
        )
        return (rd for _, _, rd in primaries)

    def test_parsers_have_same_output(self):
        unseen_steps = [
            {
                repo_name: True
                for repo_name, content in step.items()
                if content is not None  # Means "delete repo"
            }
            for step in SAMPLE_STEPS
        ]
        for repo_path, repomd in find_test_repos(self.repos_root):
            xml_rd, sql_rd = self._xml_and_sqlite_primaries(repomd)
            with open(repo_path / xml_rd.location, "rb") as xf, open(
                repo_path / sql_rd.location, "rb"
            ) as sf:
                sql_rpms = _rpm_set(sf, sql_rd)
                self.assertEqual(_rpm_set(xf, xml_rd), sql_rpms)

                # A joint test of repo parsing and `temp_repos`: check that
                # we had exactly the RPMs that were specified.
                step = int(repo_path.dirname().basename())
                repo = repo_path.basename().decode()  # `Repo` or `str` (name)
                # Resolve a string alias to the `Repo` object corresponding
                # to it.  At present, an alias refers to the repo as it
                # existed at the step that introduced the alias.  NB: These
                # semantics aren't in any way "uniquely right", it is just
                # what `temp_repos.py` does.
                search_step = step
                while isinstance(repo, str):
                    repo_name = repo
                    # Find the most recent step that defined this repo name
                    while True:
                        repo = SAMPLE_STEPS[search_step].get(repo_name)
                        if repo is not None:
                            break
                        search_step -= 1
                        assert search_step >= 0
                self.assertEqual(
                    {
                        f"rpm-test-{r.name}-{r.version}-{r.release}.x86_64.rpm"
                        for r in repo.rpms
                    },
                    {os.path.basename(r.location) for r in sql_rpms},
                    (repo, repo_name, repo_path),
                )
                unseen_steps[step].pop(repo_path.basename().decode(), None)
        self.assertEqual([], [s for s in unseen_steps if s])

    def test_pick_primary_and_errors(self):
        for _, repomd in find_test_repos(self.repos_root):
            xml_rd, sql_rd = self._xml_and_sqlite_primaries(repomd)
            self.assertIs(sql_rd, pick_primary_repodata(repomd.repodatas))
            self.assertIs(
                xml_rd,
                pick_primary_repodata(
                    [rd for rd in repomd.repodatas if rd is not sql_rd]
                ),
            )
            with self.assertRaisesRegex(RuntimeError, "^More than one primar"):
                self.assertIs(
                    xml_rd, pick_primary_repodata([sql_rd, *repomd.repodatas])
                )
            non_primary_rds = [
                rd for rd in repomd.repodatas if rd not in [sql_rd, xml_rd]
            ]
            with self.assertRaisesRegex(RuntimeError, " no known primary "):
                self.assertIs(xml_rd, pick_primary_repodata(non_primary_rds))
            with self.assertRaisesRegex(NotImplementedError, "Not reached"):
                get_rpm_parser(non_primary_rds[0])

    def test_sqlite_edge_cases(self):
        for repo_path, repomd in find_test_repos(self.repos_root):
            _, sql_rd = self._xml_and_sqlite_primaries(repomd)
            with open(repo_path / sql_rd.location, "rb") as sf:
                bz_data = sf.read()

            try:
                _rpm_set(BytesIO(bz_data + b"oops"), sql_rd)
                self.fail("Exception not raised")
            except RuntimeError as ex:
                self.assertRegex(str(ex), "^Unused data after ")
            except EOFError:
                # For reasons I don't understand, this is sometimes raised
                # instead of going down the `unused_data` branch.
                pass

            with self.assertRaisesRegex(RuntimeError, "archive is incomplete"):
                _rpm_set(BytesIO(bz_data[:-5]), sql_rd)

            # Some in-the-wild primary SQLite dbs are .gz or .xz, while
            # internally they are all are .bz2, so let's recompress.
            gzf = BytesIO()
            with gzip.GzipFile(fileobj=gzf, mode="wb") as gz_out:
                gz_out.write(bz2.decompress(bz_data))
            gzf.seek(0)
            self.assertEqual(
                _rpm_set(gzf, sql_rd._replace(location="X-primary.sqlite.gz")),
                _rpm_set(BytesIO(bz_data), sql_rd),
            )

            # Now do .xz
            xzf = BytesIO()
            with lzma.LZMAFile(filename=xzf, mode="wb") as xz_out:
                xz_out.write(bz2.decompress(bz_data))
            xzf.seek(0)
            self.assertEqual(
                _rpm_set(xzf, sql_rd._replace(location="X-primary.sqlite.xz")),
                _rpm_set(BytesIO(bz_data), sql_rd),
            )
