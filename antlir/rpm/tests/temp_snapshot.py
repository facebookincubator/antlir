#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See `temp_snapshot` below."
import os
import textwrap

from antlir.fs_utils import Path, populate_temp_dir_and_rename, temp_dir
from antlir.rpm.common import RpmShard
from antlir.rpm.snapshot_repos import snapshot_repos
from antlir.rpm.tests.temp_repos import Repo, Rpm, SAMPLE_STEPS, temp_repos_steps


def _make_test_yum_dnf_conf(yum_dnf: str, repos_path: Path, gpg_key_path: Path) -> str:
    return (
        textwrap.dedent(
            f"""\
        [main]
        debuglevel=2
        keepcache=1
        logfile=/var/log/{yum_dnf}.log
        pkgpolicy=newest
        showdupesfromrepos=1
        gpgcheck=1
        localpkg_gpgcheck=0
    """
        )
        + "\n\n".join(
            textwrap.dedent(
                f"""\
            [{repo}]
            baseurl={(repos_path / repo).file_url()}
            enabled=1
            name={repo}
            gpgkey={gpg_key_path.file_url()}
        """
            )
            for repo in repos_path.listdir()
            if repo not in (b"dnf.conf", b"yum.conf")
        )
    )


def make_temp_snapshot(
    repos, out_dir, gpg_signing_key, gpg_key_path, gpg_key_allowlist_dir
) -> Path:
    "Generates temporary RPM repo snapshots for tests to use as inputs."
    snapshot_dir = out_dir / "temp_snapshot_dir"
    os.mkdir(snapshot_dir)

    with temp_repos_steps(
        repo_change_steps=[repos], gpg_signing_key=gpg_signing_key
    ) as repos_root:
        snapshot_repos(
            dest=snapshot_dir,
            # `SnapshotReposTestCase` covers multi-universe handling
            repo_to_universe=lambda _repo: "mammal",
            # Snapshot the 0th step only, since only that is defined
            yum_conf_content=_make_test_yum_dnf_conf(
                "yum", repos_root / "0", gpg_key_path
            ),
            dnf_conf_content=_make_test_yum_dnf_conf(
                "dnf", repos_root / "0", gpg_key_path
            ),
            db_cfg={"kind": "sqlite", "db_path": out_dir / "db.sqlite3"},
            storage_cfg={
                "key": "test",
                "kind": "filesystem",
                "base_dir": out_dir / "storage",
            },
            rpm_shard=RpmShard(shard=0, modulo=1),
            gpg_key_allowlist_dir=gpg_key_allowlist_dir,
            exclude_repos=frozenset(),
            exclude_rpms=frozenset(),
            threads=4,
        )

    # Merge the repo snapshot with the storage & RPM DB -- this makes our
    # test snapshot build target look very much like prod snapshots.
    for f in snapshot_dir.listdir():
        assert not os.path.exists(out_dir / f), f"Must not overwrite {f}"
        os.rename(snapshot_dir / f, out_dir / f)
    os.rmdir(snapshot_dir)


if __name__ == "__main__":
    import argparse

    kind_to_steps = {
        "sample-step-0": SAMPLE_STEPS[0],  # Used by most tests
        # Used to test non-default repo snapshot selection
        "non-default": {
            "cheese": Repo([Rpm("cake", "non", "default"), Rpm("cheese", "0", "0")])
        },
        "rpm-replay": {
            "cheese": Repo(
                [
                    # Rpm("mice", "0.1", "a") exists in the default repo;
                    # so we use the below rpm to exercise an upgrade case.
                    Rpm("mice", "0.2", "a"),
                    # Epoch affects rpm installer output;
                    # so we use "has-epoch" rpm to exercise that parsing case.
                    Rpm("has-epoch", "0", "0", epoch="1"),
                    # Note: the dependency ordering below is intentionally
                    # not the same as the lexigrophical order as to stregthen
                    # the testing of rpm install ordering.
                    Rpm("first", "0", "0"),
                    Rpm("second", "0", "0", requires="virtual-first-0"),
                    Rpm("third", "0", "0", requires="virtual-second-0"),
                    Rpm("fourth", "0", "0", requires="virtual-third-0"),
                    Rpm("fifth", "0", "0", requires="virtual-fourth-0"),
                ]
            )
        },
    }

    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--kind", choices=list(kind_to_steps))
    parser.add_argument("--gpg-keypair-dir", type=Path.from_argparse, required=True)
    parser.add_argument(
        "out_dir", help="Write the temporary snapshot to this directory."
    )
    args = parser.parse_args()

    with temp_dir() as no_gpg_keys_yet, populate_temp_dir_and_rename(
        args.out_dir, overwrite=False  # Buck always gives us a clean workspace
    ) as td:
        signing_key_path = args.gpg_keypair_dir / "private.key"
        assert os.path.exists(
            signing_key_path
        ), f"{args.gpg_keypair_dir} must contain private.key"

        with open(signing_key_path) as key:
            gpg_signing_key = key.read()

        gpg_key_path = args.gpg_keypair_dir / "public.key"
        assert os.path.exists(
            gpg_key_path
        ), f"{args.gpg_keypair_dir} must contain public.key"

        make_temp_snapshot(
            kind_to_steps[args.kind],
            td,
            gpg_signing_key,
            gpg_key_path,
            args.gpg_keypair_dir,
        )
