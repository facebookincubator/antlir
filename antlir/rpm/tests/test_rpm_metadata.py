#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import os
import re
import shutil
import unittest

from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import temp_dir

from antlir.rpm.rpm_metadata import (
    _compare_values,
    _repo_query,
    compare_rpm_versions,
    RpmMetadata,
    RpmNotInstalledError,
)
from antlir.rpm.tests.temp_repos import (
    get_test_signing_key,
    Repo,
    Rpm,
    temp_repos_steps,
)
from antlir.tests.layer_resource import layer_resource_subvol


class RpmMetadataTestCase(unittest.TestCase):
    def _load_canonical_tests(self):
        STMT = re.compile(r"(.*)RPMVERCMP\(([^, ]*) *, *([^, ]*) *, *([^\)]*)\).*")

        for line in importlib.resources.open_text(
            "antlir.rpm", "version-compare-tests"
        ).readlines():
            m = STMT.match(line)
            if m:
                yield m.group(2), m.group(3), int(m.group(4))

    def test_rpm_metadata_from_subvol(self):
        layer_path = os.path.join(os.path.dirname(__file__), "child-layer")
        child_subvol = find_built_subvol(layer_path)
        ba_subvol = layer_resource_subvol(__package__, "test-build-appliance")

        a = RpmMetadata.from_subvol(child_subvol, ba_subvol, "rpm-test-mice")
        self.assertEqual(a.name, "rpm-test-mice")
        self.assertEqual(a.epoch, 0)
        self.assertEqual(a.version, "0.1")
        self.assertEqual(a.release, "a")

        # not installed
        with self.assertRaises(RpmNotInstalledError):
            a = RpmMetadata.from_subvol(child_subvol, ba_subvol, "rpm-test-carrot")

        # subvol with no RPM DB
        layer_path = os.path.join(os.path.dirname(__file__), "hello-layer")
        hello_subvol = find_built_subvol(layer_path)
        with self.assertRaisesRegex(ValueError, " does not exist$"):
            a = RpmMetadata.from_subvol(hello_subvol, ba_subvol, "rpm-test-mice")

    def test_rpm_metadata_from_file(self):
        with temp_repos_steps(
            gpg_signing_key=get_test_signing_key(),
            repo_change_steps=[
                {"repo": Repo([Rpm("sheep", "0.3.5.beta", "l33t.deadbeef.777")])}
            ],
        ) as repos_root, temp_dir() as td:
            src_rpm_path = repos_root / (
                "0/repo/repo-pkgs/"
                + "rpm-test-sheep-0.3.5.beta-l33t.deadbeef.777.x86_64.rpm"
            )
            dst_rpm_path = td / "arbitrary_unused_name.rpm"
            shutil.copy(src_rpm_path, dst_rpm_path)
            a = RpmMetadata.from_file(dst_rpm_path)
            self.assertEqual(a.name, "rpm-test-sheep")
            self.assertEqual(a.epoch, 0)
            self.assertEqual(a.version, "0.3.5.beta")
            self.assertEqual(a.release, "l33t.deadbeef.777")

        # non-existent file
        with self.assertRaisesRegex(RuntimeError, "^Error querying RPM:"):
            a = RpmMetadata.from_file(b"idontexist.rpm")

        # missing extension
        with self.assertRaisesRegex(ValueError, " needs to end with .rpm$"):
            a = RpmMetadata.from_file(b"idontendwithdotrpm")

    def test_rpm_query_arg_check(self):
        with self.assertRaisesRegex(ValueError, "^Must pass only "):
            _repo_query(
                db_path=b"dbpath",
                package_name=None,
                package_path=b"path",
                check_output_fn="unused",
            )

    def test_rpm_compare_versions(self):
        # name mismatch
        a = RpmMetadata("test-name1", 1, "2", "3")
        b = RpmMetadata("test-name2", 1, "2", "3")
        with self.assertRaises(ValueError):
            compare_rpm_versions(a, b)

        # Taste data was generated with:
        # rpmdev-vercmp <epoch1> <ver1> <release1> <epoch2> <ver2> <release2>
        # which also uses the same Python rpm lib.
        #
        # This number of test cases is excessive but does show how interesting
        # RPM version comparisons can be.
        test_evr_data = [
            # Non-alphanumeric (except ~) are ignored for equality
            ((1, "2", "3"), (1, "2", "3"), 0),  # 1:2-3 == 1:2-3
            ((1, ":2>", "3"), (1, "-2-", "3"), 0),  # 1::2>-3 == 1:-2--3
            ((1, "2", "3?"), (1, "2", "?3"), 0),  # 1:2-?3 == 1:2-3?
            # epoch takes precedence no matter what
            ((0, "2", "3"), (1, "2", "3"), -1),  # 0:2-3 < 1:2-3
            ((1, "1", "3"), (0, "2", "3"), 1),  # 1:1-3 > 0:2-3
            # version and release trigger the real comparison rules
            ((0, "1", "3"), (0, "2", "3"), -1),  # 0:1-3 < 0:2-3
            ((0, "~2", "3"), (0, "1", "3"), -1),  # 0:~2-3 < 0:1-3
            ((0, "~", "3"), (0, "1", "3"), -1),  # 0:~-3 < 0:1-3
            ((0, "1", "3"), (0, "~", "3"), 1),  # 0:1-3 > 0:~-3
            ((0, "^1", "3"), (0, "^", "3"), 1),  # 0:^1-3 > 0:^-3
            ((0, "^", "3"), (0, "^1", "3"), -1),  # 0:^-3 < 0:^1-3
            ((0, "0333", "b"), (0, "0033", "b"), 1),  # 0:0333-b > 0:0033-b
            ((0, "0033", "b"), (0, "0333", "b"), -1),  # 0:0033-b < 0:0333-b
            ((0, "3", "~~"), (0, "3", "~~~"), 1),  # 0:3-~~ > 0:3-~~~
            ((0, "3", "~~~"), (0, "3", "~~"), -1),  # 0:3-~~~ < 0:3-~~
            ((0, "3", "~~~"), (0, "3", "~~~"), 0),  # 0:3-~~~ == 0:3-~~~
            ((0, "a2aa", "b"), (0, "a2a", "b"), 1),  # 0:a2aa-b > 0:a2a-b
            ((0, "33", "b"), (0, "aaa", "b"), 1),  # 0:33-b > 0:aaa-b
        ]

        for evr1, evr2, expected in test_evr_data:
            a = RpmMetadata("test-name", *evr1)
            b = RpmMetadata("test-name", *evr2)
            self.assertEqual(
                compare_rpm_versions(a, b),
                expected,
                f"failed: {evr1}, {evr2}, {expected}",
            )

        # Test against some more canonical tests.  These are derived from
        # actual tests used for rpm itself.
        for ver1, ver2, expected in self._load_canonical_tests():
            self.assertEqual(
                _compare_values(ver1, ver2),
                expected,
                f"failed: {ver1}, {ver2}, {expected}",
            )
