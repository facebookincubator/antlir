# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import shutil
import sqlite3
from contextlib import contextmanager
from typing import Iterator, List, Optional, Tuple
from unittest import TestCase

from antlir.fs_utils import Path, temp_dir
from antlir.rpm.allowed_versions.update_allowed_versions import log

from antlir.rpm.repo_snapshot import RepoSnapshot

# Import FB-specific implementations if available.
try:
    from antlir.rpm.allowed_versions.facebook.tests.mock_snapshots import (
        patch_snapshots,
    )
except ImportError:

    def patch_snapshots(fn):
        return fn


from antlir.rpm.allowed_versions.update_allowed_versions import (
    parse_args,
    update_allowed_versions,
)


_FOO_RPM = "foo"
_BAR_RPM = "bar"
_NON_RPM = "non"
_FOO_BAR_VER = "some_ver"
_FOO_BAR_REL = "some_rel"
_FOO_BAR_ARCH = "some_arch"
_FOO_BAR_NO_ARCH = "*"
_FOO_JSON = "foo_json"
_DUP_JSON = "dup_json"
_VSET_NAME = "test_vset"


def prepare_rpm_repo_snapshot_dir(dir_path, foo_epoch=0):
    os.makedirs(dir_path / "snapshot")
    with sqlite3.connect(dir_path / "snapshot/snapshot.sql3") as db:
        RepoSnapshot._create_sqlite_tables(db)
        db.execute(
            "INSERT INTO 'rpm' "
            "('name', 'epoch', 'version', 'release', 'arch', 'repo', "
            "'path', 'build_timestamp', 'checksum', 'size') "
            "VALUES"
            " (?, ?, ?, ?, ?, '_repo', '_foo_path', 0, '_chksum', 0),"
            " (?, ?, ?, ?, ?, '_repo', '_bar_path', 0, '_chksum', 0)",
            (_FOO_RPM, foo_epoch, _FOO_BAR_VER, _FOO_BAR_REL, _FOO_BAR_ARCH)
            + (_BAR_RPM, 0, _FOO_BAR_VER, _FOO_BAR_REL, _FOO_BAR_ARCH),
        )


def prepare_version_sets_dir(dir_path):
    os.makedirs(dir_path / _VSET_NAME)


_EMPTY_VERSIONS = {}
_STR_VERSIONS = {_FOO_BAR_NO_ARCH: [f"{_FOO_BAR_VER}-{_FOO_BAR_REL}"]}
_DICT_VERSIONS = {
    _FOO_BAR_ARCH: [
        {
            "epoch": 0,
            "version": _FOO_BAR_VER,
            "release": _FOO_BAR_REL,
        }
    ]
}


def get_vset_to_policy(version_set, policy, versions):
    return {
        version_set: {
            "policy": policy,
            "versions": versions,
        }
    }


def get_packages(
    packages_source: str, package_names: Optional[List[str]] = None
):
    if package_names is None:
        package_names = [_FOO_RPM, _BAR_RPM, _NON_RPM]
    return {
        "source": packages_source,
        "names": package_names,
    }


def prepare_package_groups_dir(dir_path, json_name, packages, vset_to_policy):
    def _package_group(packages, vset_to_policy):
        return {
            "packages": packages,
            "version_set_to_policy": vset_to_policy,
            "oncall": "some_oncall",
        }

    with open(dir_path / f"{json_name}.json", "w") as f:
        pg = _package_group(
            packages=packages,
            vset_to_policy=vset_to_policy,
        )
        json.dump(pg, f, sort_keys=True, indent=2)


# This dummy contextmanager exists only to make linter happy about its call-site
# below (otherwise, "with ..." line becomes too long)
@contextmanager
def four_temp_dirs():
    with temp_dir() as td1:
        with temp_dir() as td2:
            with temp_dir() as td3:
                with temp_dir() as td4:
                    yield (td1, td2, td3, td4)


@contextmanager
def _test_args(
    packages_source: str = "manual",
    package_names: Optional[List[str]] = None,
    version_set=_VSET_NAME,
    policy="manual",
    versions=_STR_VERSIONS,
    foo_epoch=0,
) -> Iterator[Tuple[List[str], Path]]:
    with four_temp_dirs() as (
        data_snapshot_dir,
        package_groups_dir,
        version_sets_dir,
        rpm_repo_snaphot_dir,
    ):
        prepare_package_groups_dir(
            dir_path=package_groups_dir,
            json_name=_FOO_JSON,
            packages=get_packages(packages_source, package_names),
            vset_to_policy=get_vset_to_policy(version_set, policy, versions),
        )
        prepare_version_sets_dir(version_sets_dir)
        prepare_rpm_repo_snapshot_dir(rpm_repo_snaphot_dir, foo_epoch=foo_epoch)
        args = [
            "--data-snapshot-dir",
            data_snapshot_dir,
            "--package-groups-dir",
            package_groups_dir,
            "--version-sets-dir",
            version_sets_dir,
            "--rpm-repo-snapshot",
            rpm_repo_snaphot_dir,
            "--flavor",
            "some_flavor",
        ]
        yield (args, version_sets_dir)


_FOO_BAR_OUTPUT = f"""0\t{_BAR_RPM}\t{_FOO_BAR_VER}\t{_FOO_BAR_REL}\t{_FOO_BAR_ARCH}
0\t{_FOO_RPM}\t{_FOO_BAR_VER}\t{_FOO_BAR_REL}\t{_FOO_BAR_ARCH}"""

_RESULT_VSET_JSON = f"{_VSET_NAME}/rpm/some_oncall/{_FOO_JSON}"


class UpdateAllowedVersionsTestCase(TestCase):
    # evra.epoch is None in _resolve_envras_for_package_group()
    @patch_snapshots
    def test_versions_str(self):
        with _test_args() as (args, output_dir):
            with self.assertLogs(log) as log_ctx:
                update_allowed_versions(parse_args(args))
                with open(output_dir / _RESULT_VSET_JSON) as f:
                    self.assertEqual(f.read().strip(), _FOO_BAR_OUTPUT)
            self.assertTrue(
                any(
                    f"INFO:antlir.update_allowed_versions:The package "
                    f"{_NON_RPM} from vpgroup {_FOO_JSON} does not exist in "
                    "snapshot" in o
                    for o in log_ctx.output
                )
            )

    # evra.epoch is not None in _resolve_envras_for_package_group()
    @patch_snapshots
    def test_versions_dict(self):
        with _test_args(versions=_DICT_VERSIONS) as (args, output_dir):
            update_allowed_versions(parse_args(args))
            with open(output_dir / _RESULT_VSET_JSON) as f:
                self.assertEqual(f.read().strip(), _FOO_BAR_OUTPUT)

    # _load_package_names() raises RuntimeError if fails to load package_group
    @patch_snapshots
    def test_wrong_packages_source(self):
        with _test_args(packages_source="WRONG") as (args, output_dir):
            parsed_args = parse_args(args)
            with self.assertRaisesRegex(RuntimeError, "^Loading config "):
                update_allowed_versions(parsed_args)

    # _load_version_sets() should work if the version set directory is missing
    @patch_snapshots
    def test_missing_version_set(self):
        with _test_args(versions=_DICT_VERSIONS) as (args, output_dir):
            parsed_args = parse_args(args)
            shutil.rmtree(parsed_args.version_sets_dir)
            update_allowed_versions(parsed_args)
            with open(output_dir / _RESULT_VSET_JSON) as f:
                self.assertEqual(f.read().strip(), _FOO_BAR_OUTPUT)

    # _load_version_sets() should work if the version set directory is empty
    @patch_snapshots
    def test_empty_version_set(self):
        with _test_args(versions=_DICT_VERSIONS) as (args, output_dir):
            parsed_args = parse_args(args)
            shutil.rmtree(parsed_args.version_sets_dir)
            os.mkdir(parsed_args.version_sets_dir)
            update_allowed_versions(parsed_args)
            with open(output_dir / _RESULT_VSET_JSON) as f:
                self.assertEqual(f.read().strip(), _FOO_BAR_OUTPUT)

    # _load_version_sets() raises RuntimeError if
    # _load_policy_versions_for_packages() fails
    @patch_snapshots
    def test_wrong_policy(self):
        with _test_args(policy="WRONG") as (args, output_dir):
            parsed_args = parse_args(args)
            with self.assertRaisesRegex(RuntimeError, "^Loading config "):
                update_allowed_versions(parsed_args)

    # _load_version_sets() raises RuntimeError if a package was already added to
    # this version set by another package group config
    @patch_snapshots
    def test_duplicate_package(self):
        with _test_args() as (args, output_dir):
            fixed_index = 2
            # This option has fixed position in the list, see _test_args() above
            self.assertEqual(args[fixed_index], "--package-groups-dir")
            package_groups_dir = args[fixed_index + 1]
            self.assertIsInstance(package_groups_dir, Path)
            prepare_package_groups_dir(
                dir_path=package_groups_dir,
                json_name=_DUP_JSON,
                packages=get_packages("manual"),
                vset_to_policy=get_vset_to_policy(
                    _VSET_NAME, "manual", _STR_VERSIONS
                ),
            )
            parsed_args = parse_args(args)
            with self.assertRaisesRegex(RuntimeError, "^Loading config "):
                update_allowed_versions(parsed_args)

    # _resolve_envras_for_package_group() returns empty set if not vpgroup.evras
    @patch_snapshots
    def test_no_versions(self):
        with _test_args(versions=_EMPTY_VERSIONS) as (args, output_dir):
            with self.assertLogs(log) as log_ctx:
                update_allowed_versions(parse_args(args))
                self.assertFalse(os.path.exists(output_dir / _RESULT_VSET_JSON))
            self.assertTrue(
                any(
                    "ERROR:antlir.update_allowed_versions:"
                    "XXX group has no lock in any snapshot" in o
                    for o in log_ctx.output
                )
            )

    # _resolve_envras_for_package_group() tries to find EVRs that are valid for
    # all packages in the group, foo_epoch=1 makes it impossible
    @patch_snapshots
    def test_no_suitable_evr(self):
        with _test_args(foo_epoch=1) as (args, output_dir):
            with self.assertLogs(log) as log_ctx:
                update_allowed_versions(parse_args(args))
                self.assertFalse(os.path.exists(output_dir / _RESULT_VSET_JSON))
            self.assertTrue(
                any(
                    "ERROR:antlir.update_allowed_versions:"
                    "XXX group has no lock in any snapshot" in o
                    for o in log_ctx.output
                )
            )

    # _resolve_envras_for_package_group() returns empty set if there are no
    # rpms with a matching name
    @patch_snapshots
    def test_no_name_match(self):
        with _test_args(package_names=[_NON_RPM]) as (args, output_dir):
            with self.assertLogs(log) as log_ctx:
                update_allowed_versions(parse_args(args))
                self.assertFalse(os.path.exists(output_dir / _RESULT_VSET_JSON))
            self.assertTrue(
                any(
                    "ERROR:antlir.update_allowed_versions:"
                    "XXX group has no lock in any snapshot" in o
                    for o in log_ctx.output
                )
            )
