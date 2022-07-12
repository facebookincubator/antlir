#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import sqlite3
import tempfile
import unittest
import unittest.mock
from typing import FrozenSet

from antlir.fs_utils import Path, temp_dir

from antlir.rpm import repo_db
from antlir.rpm.repo_snapshot import RepoSnapshot
from antlir.rpm.snapshot_repos import snapshot_repos_from_args
from antlir.rpm.storage import Storage
from antlir.rpm.tests import temp_repos


def _read_conf_headers(conf_path: Path) -> FrozenSet[str]:
    with open(conf_path) as f:
        header_list = [l.strip() for l in f.readlines() if l.startswith("[")]
        headers = frozenset(header_list)
        assert len(headers) == len(header_list), header_list
        return headers


class SnapshotReposTestCase(unittest.TestCase):
    def setUp(self):
        self.maxDiff = 12345

    def test_invalid_json(self):
        args = [
            "--gpg-key-allowlist-dir",
            ("not_used/gpg_allowlist"),
            "--storage",
            json.dumps(
                {"key": "test", "kindd": "filesystem", "base_dir": "foo"}
            ),
            "--db",
            json.dumps({"kind": "sqlite", "db_path": "foo_path"}),
            "--one-universe-for-all-repos",
            "not_used",
            "--threads",
            "4",
            "--dnf-conf",
            "not_used",
            "--yum-conf",
            "not_used",
            "--snapshot-dir",
            "not_used",
        ]
        with self.assertRaises(KeyError):
            snapshot_repos_from_args(args)

        args[3] = json.dumps(
            {"key": "test", "kindd": "filesystem", "base_dir": "foo"}
        )
        args[5] = json.dumps({"kind": "sqlite", "bd_path": "bad_key"})
        with self.assertRaises(KeyError):
            snapshot_repos_from_args(args)

    def test_snapshot(self):
        with temp_repos.temp_repos_steps(
            gpg_signing_key=temp_repos.get_test_signing_key(),
            repo_change_steps=[
                {  # All of the `snap0` repos are in the "mammal" universe
                    "bunny": temp_repos.SAMPLE_STEPS[0]["bunny"],
                    "cat": temp_repos.SAMPLE_STEPS[0]["cat"],
                    "dog": temp_repos.SAMPLE_STEPS[0]["dog"],
                    "kitteh": "cat",
                    "gonna_skip_for_0": "bunny",
                },
                # None of these are in the "mammal" universe, see `ru_json`
                # below.
                {
                    # 'bunny' stays unchanged, with the step 0 `repomd.xml`
                    "cat": temp_repos.SAMPLE_STEPS[1]["cat"],
                    "dog": temp_repos.SAMPLE_STEPS[1]["dog"],
                    # 'kitteh' stays unchanged, with the step 0 `repomd.xml`
                },
            ],
        ) as repos_root, temp_dir() as td:
            storage_dict = {
                "key": "test",
                "kind": "filesystem",
                "base_dir": td / "storage",
            }
            repo_db_path = td / "db.sqlite3"

            # Mock all repomd fetch timestamps to be identical to test that
            # multiple universes do not collide.
            orig_store_repomd = repo_db.RepoDBContext.store_repomd
            with unittest.mock.patch.object(
                repo_db.RepoDBContext,
                "store_repomd",
                lambda self, universe_s, repo_s, repomd: orig_store_repomd(
                    self,
                    universe_s,
                    repo_s,
                    repomd._replace(fetch_timestamp=451),
                ),
            ), tempfile.NamedTemporaryFile("w") as ru_json:
                common_args = [
                    f'--gpg-key-allowlist-dir={td / "gpg_allowlist"}',
                    "--storage=" + Path.json_dumps(storage_dict),
                    "--db="
                    + Path.json_dumps(
                        {"kind": "sqlite", "db_path": repo_db_path}
                    ),
                    "--threads=4",
                ]
                snapshot_repos_from_args(
                    common_args
                    + [
                        "--one-universe-for-all-repos=mammal",
                        f'--dnf-conf={repos_root / "0/dnf.conf"}',
                        f'--yum-conf={repos_root / "0/yum.conf"}',
                        f'--snapshot-dir={td / "snap0"}',
                        "--exclude=gonna_skip_for_0",
                    ]
                )
                # We want to avoid involving the "mammal" universe to
                # exercise the fact that a universe **not** mentioned in a
                # snapshot is not used for mutable RPM detection.  The fact
                # that we also have a "zombie" exercises the fact that we do
                # detect cross-universe mutable RPMs when the universes
                # occur in the same snapshot.  Search below for
                # `rpm-test-mutable` and `rpm-test-milk`.
                json.dump(
                    {
                        "bunny": "marsupial",  # Same content as in snap0
                        "cat": "zombie",  # Changes content from snap0
                        "dog": "marsupial",  # Changes content from snap0
                        "kitteh": "marsupial",  # Same content as in snap0
                        "gonna_skip_for_0": "thisone",
                    },
                    ru_json,
                )
                ru_json.flush()
                snapshot_repos_from_args(
                    common_args
                    + [
                        # Don't specify yum.conf since it's going away.
                        f"--repo-to-universe-json={ru_json.name}",
                        f'--dnf-conf={repos_root / "1/dnf.conf"}',
                        f'--snapshot-dir={td / "snap1"}',
                    ]
                )

            updated_headers = {}
            orig_headers = {}
            for snap, conf_type in zip(["snap0", "snap1"], ["yum", "dnf"]):
                updated_path = td / snap / f"{conf_type}.conf"
                orig_path = Path(updated_path + b".original")
                updated_headers[snap] = _read_conf_headers(updated_path)
                orig_headers[snap] = _read_conf_headers(orig_path)

            # For snap0, orig file should differ from conf since we excluded
            excluded = "[gonna_skip_for_0]"
            self.assertNotIn(excluded, updated_headers["snap0"])
            self.assertEqual(
                updated_headers["snap0"] | {excluded}, orig_headers["snap0"]
            )
            # For snap1, orig file should equal conf
            self.assertEqual(updated_headers["snap1"], orig_headers["snap1"])

            with sqlite3.connect(repo_db_path) as db:
                # Check that repomd rows are repeated or duplicated as we'd
                # expect across `snap[01]`, and the universes.
                repo_mds = sorted(
                    db.execute(
                        """
                    SELECT "universe", "repo", "fetch_timestamp", "checksum"
                    FROM "repo_metadata"
                """
                    ).fetchall()
                )
                self.assertEqual(
                    [
                        ("mammal", "bunny", 451),  # snap0
                        ("mammal", "cat", 451),  # snap0
                        ("mammal", "dog", 451),  # snap0
                        ("mammal", "kitteh", 451),  # snap0 -- index -5
                        ("marsupial", "bunny", 451),  # snap1
                        ("marsupial", "dog", 451),  # snap1
                        ("marsupial", "kitteh", 451),  # snap1 -- index -2
                        ("thisone", "gonna_skip_for_0", 451),  # snap1
                        ("zombie", "cat", 451),  # snap1
                    ],
                    [r[:3] for r in repo_mds],
                )
                # The kittehs have the same checksums, but exist separately
                # due to being in different universes.
                self.assertEqual(repo_mds[-3][1:], repo_mds[-6][1:])

                def _fetch_sorted_by_nevra(nevra):
                    return sorted(
                        db.execute(
                            """
                    SELECT "universe", "name", "epoch", "version",
                        "release", "arch", "checksum"
                    FROM "rpm"
                    WHERE "name" = ? AND "epoch" = ? AND "version" = ? AND
                        "release" = ? AND "arch" = ?
                    """,
                            nevra,
                        ).fetchall()
                    )

                # We expect this identical "carrot" RPM (same checksums) to
                # be repeated because it occurs in two different universes.
                kitteh_carrot_nevra = [
                    "rpm-test-carrot",
                    0,
                    "1",
                    "lockme",
                    "x86_64",
                ]
                kitteh_carrots = _fetch_sorted_by_nevra(kitteh_carrot_nevra)
                kitteh_carrot_chksum = kitteh_carrots[0][-1]
                self.assertEqual(
                    [
                        # step0 cat & kitteh
                        ("mammal", *kitteh_carrot_nevra, kitteh_carrot_chksum),
                        # step1 kitteh
                        (
                            "marsupial",
                            *kitteh_carrot_nevra,
                            kitteh_carrot_chksum,
                        ),
                    ],
                    kitteh_carrots,
                )

                # This RPM has two variants for its contents at step 1.
                # This creates a mutable RPM error in `snap1`.  Note that we
                # detect it even though the variants are in different
                # universes.
                milk2_nevra = ["rpm-test-milk", 0, "2.71", "8", "x86_64"]
                milk2s = _fetch_sorted_by_nevra(milk2_nevra)
                milk2_chksum_step0 = milk2s[0][-1]  # mammal sorts first
                (milk2_chksum_step1,) = {milk2s[1][-1], milk2s[2][-1]} - {
                    milk2_chksum_step0
                }
                self.assertEqual(
                    [
                        # snap0 cat & kitteh
                        ("mammal", *milk2_nevra, milk2_chksum_step0),
                        # snap1 kitteh -- mutable RPM error vs "snap1 cat"
                        ("marsupial", *milk2_nevra, milk2_chksum_step0),
                        # snap1 cat -- mutable RPM error vs "snap1 kitteh"
                        ("zombie", *milk2_nevra, milk2_chksum_step1),
                    ],
                    milk2s,
                )

                # This RPM changes contents between step 0 and step 1, but
                # since the "mammal" universe is not used in step 1, there
                # is no mutable RPM error.
                mutable_nevra = ["rpm-test-mutable", 0, "a", "f", "x86_64"]
                mutables = _fetch_sorted_by_nevra(mutable_nevra)
                mutable_chksum_dog = mutables[0][-1]  # mammal sorts first
                mutable_chksum_cat = mutables[1][-1]
                self.assertEqual(
                    sorted(
                        [
                            # snap0 dog
                            ("mammal", *mutable_nevra, mutable_chksum_dog),
                            # snap1 cat
                            ("zombie", *mutable_nevra, mutable_chksum_cat),
                        ]
                    ),
                    mutables,
                )

            # As with `test_snapshot_repo`, this is not a complete check of
            # the snapshot state.  We only check for sanity, and for the
            # interactions between multiple snapshots & multiple universes.
            # Lower-level tests check many other lower-level details.
            mutable_a_f_checksums = set()
            milk2_checksums = set()
            expected_errors = 1
            for snap_name, expected_rows in [
                # These are just straight up "bunny", "cat" (with alias),
                # and "dog" from SAMPLE_STEPS[0], as indicated in our setup.
                (
                    "snap0",
                    {
                        ("bunny", "bunny-pkgs/rpm-test-carrot-2-rc0"),
                        ("bunny", "bunny-pkgs/rpm-test-veggie-2-rc0"),
                        ("cat", "cat-pkgs/rpm-test-carrot-1-lockme"),
                        ("cat", "cat-pkgs/rpm-test-mice-0.1-a"),
                        ("cat", "cat-pkgs/rpm-test-milk-2.71-8"),
                        ("cat", "cat-pkgs/rpm-test-milk-no-sh-v-r"),
                        ("cat", "cat-pkgs/rpm-test-veggie-1-rc0"),
                        ("dog", "dog-pkgs/rpm-test-carrot-2-rc0"),
                        ("dog", "dog-pkgs/rpm-test-etc-dnf-macro-1-2"),
                        ("dog", "dog-pkgs/rpm-test-etc-yum-macro-1-2"),
                        ("dog", "dog-pkgs/rpm-test-mice-0.1-a"),
                        ("dog", "dog-pkgs/rpm-test-milk-1.41-42"),
                        ("dog", "dog-pkgs/rpm-test-mutable-a-f"),
                        ("kitteh", "cat-pkgs/rpm-test-carrot-1-lockme"),
                        ("kitteh", "cat-pkgs/rpm-test-mice-0.1-a"),
                        ("kitteh", "cat-pkgs/rpm-test-milk-2.71-8"),
                        ("kitteh", "cat-pkgs/rpm-test-milk-no-sh-v-r"),
                        ("kitteh", "cat-pkgs/rpm-test-veggie-1-rc0"),
                    },
                ),
                # These are "bunny" & "cat" (as "kitteh") from
                # SAMPLE_STEPS[0], plus "cat" & "dog from SAMPLE_STEPS[1].
                #
                (
                    "snap1",
                    {
                        ("bunny", "bunny-pkgs/rpm-test-carrot-2-rc0"),
                        ("bunny", "bunny-pkgs/rpm-test-veggie-2-rc0"),
                        ("cat", "cat-pkgs/rpm-test-milk-2.71-8"),  # may error
                        ("cat", "cat-pkgs/rpm-test-mice-0.2-rc0"),
                        # We'd have gotten a "mutable RPM" error if this
                        # were in the same universe as the "mutable" from
                        # "dog" in snap0.
                        ("cat", "cat-pkgs/rpm-test-mutable-a-f"),
                        ("dog", "dog-pkgs/rpm-test-carrot-2-rc0"),
                        ("dog", "dog-pkgs/rpm-test-bone-5i-beef"),
                        (
                            "gonna_skip_for_0",
                            "bunny-pkgs/rpm-test-carrot-2-rc0",
                        ),
                        (
                            "gonna_skip_for_0",
                            "bunny-pkgs/rpm-test-veggie-2-rc0",
                        ),
                        ("kitteh", "cat-pkgs/rpm-test-carrot-1-lockme"),
                        ("kitteh", "cat-pkgs/rpm-test-veggie-1-rc0"),
                        ("kitteh", "cat-pkgs/rpm-test-mice-0.1-a"),
                        (
                            "kitteh",
                            "cat-pkgs/rpm-test-milk-2.71-8",
                        ),  # may error
                        ("kitteh", "cat-pkgs/rpm-test-milk-no-sh-v-r"),
                    },
                ),
            ]:
                with sqlite3.connect(
                    RepoSnapshot.fetch_sqlite_from_storage(
                        Storage.make(**storage_dict),
                        td / snap_name,
                        td / snap_name / "snapshot.sql3",
                    )
                ) as db:
                    rows = db.execute(
                        'SELECT "repo", "path", "error", "checksum" FROM "rpm"'
                    ).fetchall()
                    self.assertEqual(
                        {(r, p + ".x86_64.rpm") for r, p in expected_rows},
                        {(r, p) for r, p, _e, _c in rows},
                    )
                    for repo, path, error, chksum in rows:
                        # There is just 1 error among all the rows.  The
                        # "milk-2.71" RPM from either "kitteh" or "cat" in
                        # `snap1` gets marked with "mutable_rpm".  Which
                        # repo gets picked depends on the (shuffled) order
                        # of the snapshot.  If we were to run the `snap1`
                        # snapshot a second time, both would get marked.
                        if error is not None:
                            expected_errors -= 1
                            self.assertEqual(
                                (
                                    "snap1",
                                    "cat-pkgs/rpm-test-milk-2.71-8.x86_64.rpm",
                                    "mutable_rpm",
                                ),
                                (snap_name, path, error),
                                repo,
                            )
                            self.assertIn(repo, {"cat", "kitteh"})
                        # Sanity-check checksums
                        self.assertTrue(chksum.startswith("sha384:"), chksum)
                        if path == "cat-pkgs/rpm-test-milk-2.71-8.x86_64.rpm":
                            milk2_checksums.add(chksum)
                        if path.endswith("rpm-test-mutable-a-f.x86_64.rpm"):
                            mutable_a_f_checksums.add(chksum)

            self.assertEqual(0, expected_errors)
            self.assertEqual(2, len(milk2_checksums))
            self.assertEqual(2, len(mutable_a_f_checksums))
