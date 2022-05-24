#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest
from contextlib import AbstractContextManager
from io import BytesIO
from unittest import mock

from antlir.fs_utils import Path, temp_dir

from ..common import (
    Checksum,
    DecorateContextEntry,
    has_yum,
    read_chunks,
    readonly_snapshot_db,
    RpmShard,
    yum_is_dnf,
)


class TestCommon(unittest.TestCase):
    def test_readonly_snapshot_db(self) -> None:
        with self.assertRaises(FileNotFoundError):
            readonly_snapshot_db(Path("/DoEs/nOt/eXiSt"))
        with readonly_snapshot_db(
            Path(os.environ["antlir_test_snapshot"])
        ) as db:
            ((rows,),) = db.execute("SELECT COUNT(1) FROM `rpm`").fetchall()
            self.assertGreaterEqual(rows, 1)

    def test_rpm_shard(self) -> None:
        self.assertEqual(
            RpmShard(shard=3, modulo=7), RpmShard.from_string("3:7")
        )

        class FakeRpm:
            def __init__(self, nevra):
                self._nevra = nevra

            def nevra(self):
                return self._nevra

        self.assertEqual(
            [("foo", True), ("bar", False), ("foo", False), ("bar", True)],
            [
                (rpm, shard.in_shard(FakeRpm(rpm)))
                for shard in [RpmShard(1, 7), RpmShard(2, 7)]
                for rpm in ["foo", "bar"]
            ],
        )

    def test_checksum(self) -> None:
        cs = Checksum(algorithm="oops", hexdigest="dada")
        self.assertEqual("oops:dada", str(cs))
        self.assertEqual(cs, Checksum.from_string(str(cs)))
        for algo in ["sha1", "sha"]:
            h = Checksum(algo, "ignored").hasher()
            h.update(b"banana")
            self.assertEqual(
                "250e77f12a5ab6972a0895d290c4792f0a326ea8", h.hexdigest()
            )

    def test_read_chunks(self) -> None:
        self.assertEqual(
            [b"first", b"secon", b"d"],
            list(read_chunks(BytesIO(b"firstsecond"), 5)),
        )

    def test_has_yum(self) -> None:
        with mock.patch("shutil.which") as mock_which:
            mock_which.return_value = "/path/to/yum"
            self.assertTrue(has_yum())
            mock_which.return_value = None
            self.assertFalse(has_yum())

    def test_yum_is_dnf(self) -> None:
        # Setup for yum not being the same as dnf, modeled after fb
        with temp_dir() as td:
            yum_path = Path(td / "yum").touch()

            with mock.patch("shutil.which") as mock_which:
                mock_which.return_value = None
                self.assertFalse(yum_is_dnf())
                mock_which.return_value = yum_path.decode()
                self.assertFalse(yum_is_dnf())

        # Setup for yum being the same as dnf, modeled after fedora
        # where `/bin/yum -> dnf-3`
        with temp_dir() as td:
            dnf_name = "dnf-3"
            dnf_path = Path(td / dnf_name).touch()
            yum_path = td / "yum"
            # Symlink to the name for a relative symlink that ends up
            # as yum -> dnf-3
            os.symlink(dnf_name, yum_path)

            with mock.patch("shutil.which") as mock_which:
                mock_paths = {dnf_name: dnf_path, "yum": yum_path}
                mock_which.side_effect = lambda p: mock_paths[p].decode()

                self.assertTrue(yum_is_dnf())

    def test_decorate_context_entry(self) -> None:
        class ExCustomErr(Exception):
            pass

        # pyre-fixme[3]: Return type must be annotated.
        # pyre-fixme[2]: Parameter must be annotated.
        def catch_it(fn):
            """Decorator that simply catches `ExCustomErr` and returns whether
            an error was raised by `fn` or not.
            """

            # pyre-fixme[53]: Captured variable `fn` is not annotated.
            # pyre-fixme[3]: Return type must be annotated.
            # pyre-fixme[2]: Parameter must be annotated.
            def decorated(*args, **kwargs):
                try:
                    fn(*args, **kwargs)
                    return "entered"
                except ExCustomErr:
                    return "caught"

            return decorated

        class ExCtxMgr(AbstractContextManager):
            def __init__(self, will_raise: bool):
                self.will_raise = will_raise

            def __enter__(self):
                if self.will_raise:
                    raise ExCustomErr

            def __exit__(self, *args, **kwargs):
                pass

        # Ensure our test context manager is working as expected
        with self.assertRaises(ExCustomErr):
            with ExCtxMgr(True):
                pass

        # Ensure our manager properly applies decorator on entry
        with DecorateContextEntry(ExCtxMgr(True), catch_it) as res:
            self.assertEqual(res, "caught")
        with DecorateContextEntry(ExCtxMgr(False), catch_it) as res:
            self.assertEqual(res, "entered")
