# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import shutil
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

# CentOS 10 keeps the RPM database under /usr/lib/sysimage/rpm, CentOS 9 under
# /var/lib/rpm.
RPMDB_DIRS = ["usr/lib/sysimage/rpm", "var/lib/rpm"]
HISTORY = "var/lib/dnf/history.sqlite"
# dnf history records the *layer* label in trans.cmdline. The two layers here
# differ only in that label, and those bytes also shift Hermit's virtual clock
# enough to move trans.dt_end. Two builds of the same layer match in every
# column (check_reproducible.sh).
LABEL_DEPENDENT = {("trans", "cmdline"), ("trans", "dt_end")}


def rows(db_path: Path, tmp: str, side: str) -> dict[str, tuple[list[str], list]]:
    # Read a copy, with its -wal/-shm, so the layer stays untouched.
    dst = Path(tmp) / f"{side}.sqlite"
    for suffix in ("", "-wal", "-shm"):
        src = Path(str(db_path) + suffix)
        if src.exists():
            shutil.copyfile(src, str(dst) + suffix)
    out = {}
    with closing(sqlite3.connect(dst)) as db:
        for (table,) in db.execute("SELECT name FROM sqlite_master WHERE type='table'"):
            cols = [r[1] for r in db.execute(f"PRAGMA table_info({table!r})")]
            out[table] = (
                cols,
                db.execute(f"SELECT * FROM {table!r} ORDER BY rowid").fetchall(),
            )
    return out


class TestHermeticRpm(unittest.TestCase):
    def setUp(self) -> None:
        self.a = Path(os.environ["LAYER_A"])
        self.b = Path(os.environ["LAYER_B"])

    def test_rpmdb_is_identical(self) -> None:
        files = [
            f"{d}/rpmdb.sqlite{suffix}"
            for d in RPMDB_DIRS
            for suffix in ("", "-wal", "-shm")
            if (self.a / d / f"rpmdb.sqlite{suffix}").exists()
            or (self.b / d / f"rpmdb.sqlite{suffix}").exists()
        ]
        self.assertTrue(any(f.endswith("rpmdb.sqlite") for f in files), files)
        for path in files:
            with self.subTest(path=path):
                self.assertEqual(
                    (self.a / path).read_bytes(), (self.b / path).read_bytes()
                )

    def test_dnf_history_is_identical(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            a = rows(self.a / HISTORY, tmp, "a")
            b = rows(self.b / HISTORY, tmp, "b")
        self.assertEqual(sorted(a), sorted(b))
        self.assertTrue(a["trans"][1], "no dnf transactions recorded")
        for table, (cols, a_rows) in a.items():
            b_cols, b_rows = b[table]
            self.assertEqual(cols, b_cols, table)
            self.assertEqual(len(a_rows), len(b_rows), table)
            for a_row, b_row in zip(a_rows, b_rows, strict=True):
                for col, x, y in zip(cols, a_row, b_row, strict=True):
                    if (table, col) not in LABEL_DEPENDENT:
                        with self.subTest(table=table, column=col):
                            self.assertEqual(x, y)
