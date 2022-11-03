#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import errno
import os
import subprocess
import sys
import tempfile
import unittest.mock

from antlir.artifacts_dir import ensure_per_repo_artifacts_dir_exists

from antlir.btrfs_diff.tests.demo_sendstreams_expected import (
    render_demo_as_corrupted_by_gnu_tar,
    render_demo_subvols,
)
from antlir.btrfsutil import BtrfsUtilError
from antlir.fs_utils import Path, temp_dir
from antlir.subvol_utils import (
    find_subvolume_on_disk,
    KiB,
    Subvol,
    TempSubvolumes,
    volume_dir,
    with_temp_subvols,
)
from antlir.tests.common import AntlirTestCase
from antlir.tests.subvol_helpers import render_subvol
from antlir.volume_for_repo import get_volume_for_current_repo


class SubvolTestCase(AntlirTestCase):
    """
    NB: The test here is partially redundant with demo_sendstreams, but
    coverage easier to manage when there's a clean, separate unit test.
    """

    def setUp(self):
        super().setUp()
        # Make sure we have a volume to work with
        get_volume_for_current_repo(
            ensure_per_repo_artifacts_dir_exists(Path(sys.argv[0]))
        )

    @with_temp_subvols
    def test_create_and_snapshot_and_already_exists(self, temp_subvols):
        p = temp_subvols.create("parent")
        p2 = Subvol(p.path(), already_exists=True)
        self.assertEqual(p.path(), p2.path())
        temp_subvols.snapshot(p2, "child")

    def _assert_subvol_exists(self, subvol):
        self.assertTrue(subvol._exists)
        self.assertTrue(subvol.path().exists(), subvol.path())

    def _assert_subvol_does_not_exist(self, subvol):
        self.assertFalse(subvol._exists)
        self.assertFalse(subvol.path().exists(), subvol.path())

    # Checks that `_mark_{created,deleted}` work as expected, including
    # nested subvolumes, read-only subvolumes, and `Subvol`s that are clones
    # of one another.
    @with_temp_subvols
    def test_create_and_delete_on_exit(self, temp_subvols):
        s1 = temp_subvols.caller_will_create("create_and_delete")
        self._assert_subvol_does_not_exist(s1)

        with s1.create().delete_on_exit():
            self._assert_subvol_exists(s1)

            with Subvol(s1.path(), already_exists=True).delete_on_exit() as s2:
                self._assert_subvol_exists(s2)
                self._assert_subvol_exists(s2)

                nested = Subvol(s1.path("nested")).create()
                # Cover the "nested untracked subvol" case.  This deliberately
                # omits `Subvol.maybe_create_externally()`.
                s1.run_as_root(
                    ["btrfs", "subvolume", "create", s1.path("nested_extern")],
                )

                self._assert_subvol_exists(nested)
                self.assertTrue(os.path.exists(s1.path("nested_extern")))

                # Verify that parents get marked RW before deleting children.
                s2.set_readonly(True)  # is a clone of `s1`
                # It's OK if a child is read-only, too.
                nested.set_readonly(True)

            self._assert_subvol_does_not_exist(s1)
            self._assert_subvol_does_not_exist(s2)
            self._assert_subvol_does_not_exist(nested)

    @with_temp_subvols
    def test_maybe_create_externally(self, temp_subvols):
        sv = temp_subvols.caller_will_create("maybe_create_externally")

        with sv.maybe_create_externally():
            pass  # We didn't create the subvol, and that's OK
        self.assertFalse(sv._exists)

        with sv.delete_on_exit():
            with sv.maybe_create_externally():
                sv.run_as_root(
                    ["btrfs", "subvolume", "create", sv.path()],
                    _subvol_exists=False,
                )
            self.assertTrue(sv._exists)
        self.assertFalse(sv._exists)

    def test_does_not_exist(self):
        with tempfile.TemporaryDirectory() as td:
            with self.assertRaisesRegex(AssertionError, "No btrfs subvol"):
                Subvol(td, already_exists=True)

            sv = Subvol(td, _test_only_allow_existing=True)
            with self.assertRaisesRegex(AssertionError, "exists is False"):
                sv.run_as_root(["true"])

    def test_out_of_subvol_symlink(self):
        with temp_dir() as td:
            os.symlink("/dev/null", td / "my_null")
            sv = Subvol(td, _test_only_allow_existing=True)
            self.assertEqual(
                td / "my_null",
                sv.path("my_null", no_dereference_leaf=True),
            )
            with self.assertRaisesRegex(AssertionError, " is outside of "):
                sv.path("my_null")

    def test_run_as_root_no_cwd(self):
        sv = Subvol("/dev/null/no-such-dir")
        sv.run_as_root(["true"], _subvol_exists=False)
        with self.assertRaisesRegex(AssertionError, "cwd= is not permitte"):
            sv.run_as_root(["true"], _subvol_exists=False, cwd=".")

    def test_run_as_root_return(self):
        args = ["bash", "-c", "echo -n my out; echo -n my err >&2"]
        r = Subvol("/dev/null/no-such-dir").run_as_root(
            args,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            _subvol_exists=False,
        )
        self.assertEqual(["sudo", "TMP=", "--"] + args, r.args)
        self.assertEqual(0, r.returncode)
        self.assertEqual(b"my out", r.stdout)
        self.assertEqual(b"my err", r.stderr)

    def test_path(self):
        # We are only going to do path manipulations in this test.
        sv = Subvol("/subvol/need/not/exist")

        for bad_path in ["..", "a/../../b/c/d", "../c/d/e"]:
            with self.assertRaisesRegex(AssertionError, "is outside of"):
                sv.path(bad_path)

        self.assertEqual(sv.path("a/b"), sv.path("/a/b/"))

        self.assertEqual(b"a/b", sv.path("a/b").relpath(sv.path()))

        self.assertTrue(not sv.path(".").endswith(b"/."))

    def test_canonicalize_path(self):
        with temp_dir() as td:
            with unittest.mock.patch(
                "antlir.subvol_utils._path_is_btrfs_subvol",
                unittest.mock.Mock(side_effect=lambda path: path.startswith(td)),
            ):
                sv = Subvol(td, already_exists=True)
                os.mkdir(td / "real")
                (td / "real/file").touch()
                os.symlink("real/file", td / "indirect1")
                os.mkdir(td / "indirect2")
                os.symlink("../indirect1", td / "indirect2/link")
                self.assertEqual(b"/real/file", sv.canonicalize_path("indirect2/link"))
                self.assertEqual(b"/", sv.canonicalize_path("./."))

    @with_temp_subvols
    def test_run_as_root_input(self, temp_subvols):
        sv = temp_subvols.create("subvol")
        sv.run_as_root(["tee", sv.path("hello")], input=b"world")
        with open(sv.path("hello")) as infile:
            self.assertEqual("world", infile.read())

    @with_temp_subvols
    def test_mark_readonly_and_get_sendstream(self, temp_subvols):
        sv = temp_subvols.create("subvol")
        sv.run_as_root(["touch", sv.path("abracadabra")])
        sendstream = sv.mark_readonly_and_get_sendstream()
        self.assertIn(b"abracadabra", sendstream)
        with tempfile.TemporaryFile() as outfile:
            with sv.mark_readonly_and_write_sendstream_to_file(outfile):
                pass
            outfile.seek(0)
            self.assertEqual(sendstream, outfile.read())

    @with_temp_subvols
    def test_receive(self, temp_subvols):
        new_subvol_name = "differs_from_create_ops"
        sv = temp_subvols.caller_will_create(new_subvol_name)
        with open(Path(__file__).dirname() / "create_ops.sendstream") as f, sv.receive(
            f
        ):
            pass
        self.assertEqual(
            render_demo_subvols(create_ops=new_subvol_name), render_subvol(sv)
        )

    @with_temp_subvols
    def test_write_to_tarball(self, temp_subvols):
        # create a subvol from a demo sendstream, tar it, untar into a new
        # subvol, then compare the two
        demo_sv_name = "demo_sv"
        demo_sv = temp_subvols.caller_will_create(demo_sv_name)
        with open(
            Path(__file__).dirname() / "create_ops.sendstream"
        ) as f, demo_sv.receive(f):
            pass

        unpacked_sv = temp_subvols.create("subvol")
        with tempfile.NamedTemporaryFile() as tar_file:
            with demo_sv.write_tarball_to_file(tar_file):
                pass

            demo_sv.run_as_root(
                [
                    "tar",
                    "xf",
                    tar_file.name,
                    "--acls",
                    "--xattrs",
                    "-C",
                    unpacked_sv.path(),
                ]
            )

        demo_render = render_demo_as_corrupted_by_gnu_tar(create_ops=demo_sv_name)

        self.assertEqual(demo_render, render_subvol(unpacked_sv))

    @with_temp_subvols
    def test_equal_and_hash(self, temp_subvols):
        sv = temp_subvols.create("subvol")
        other_sv = Subvol(sv.path(), already_exists=True)

        self.assertEqual(sv, other_sv)
        self.assertEqual(sv.__hash__(), hash(sv._path))

    def test_read_file(self):
        with temp_dir() as td:
            with open(td / "test_file", "w") as f:
                f.write("foo")
            sv = Subvol(td, _test_only_allow_existing=True)
            self.assertEqual(sv.read_path_text(Path("test_file")), "foo")

    @with_temp_subvols
    def test_write_file(self, ts: TempSubvolumes):
        sv = ts.create("test_write_file")
        sv.overwrite_path_as_root(Path("test_file"), contents=b"foobytes")
        self.assertEqual(sv.path("test_file").read_text(), "foobytes")

        sv.overwrite_path_as_root(Path("test_file"), contents="foostr")
        self.assertEqual(sv.path("test_file").read_text(), "foostr")

    def test_with_temp_subvols(self):
        temp_dir_path = None

        def fn(self, ts):
            nonlocal temp_dir_path
            prefix = volume_dir(Path(sys.argv[0])) / "tmp" / "TempSubvolumes_"
            self.assertTrue(ts.temp_dir.startswith(prefix))
            self.assertTrue(os.path.exists(ts.temp_dir))
            temp_dir_path = ts.temp_dir

        with_temp_subvols(fn)(self)
        self.assertIsNotNone(temp_dir_path)
        self.assertFalse(os.path.exists(temp_dir_path))

    def test_temp_subvolumes_create(self):
        with TempSubvolumes() as ts:
            td_path = ts.temp_dir
            sv_path = ts.temp_dir / "test"
            self.assertTrue(os.path.exists(td_path))
            self.assertFalse(os.path.exists(sv_path))

            sv = ts.create("test")
            self.assertEqual(sv.path(), sv_path)
            self.assertTrue(os.path.exists(sv_path))
            self.assertTrue(sv._exists)

            # NB: Changing this to `ts.create("test/nested")` would break
            # the test because this would cause us to try to delete "nested"
            # while "test" is still read-only.
            sv_nested = Subvol(sv.path("nested")).create()
            self.assertEqual(sv_nested.path(), ts.temp_dir / "test/nested")
            self.assertTrue(os.path.exists(sv_nested.path()))
            self.assertTrue(sv_nested._exists)

            sv.set_readonly(True)  # Does not break clean-up

        self.assertIsNotNone(td_path)
        self.assertIsNotNone(sv_path)
        self.assertFalse(os.path.exists(td_path))
        self.assertFalse(os.path.exists(sv_path))

        self.assertFalse(sv._exists)
        self.assertFalse(sv_nested._exists)

    def test_temp_subvolumes_snapshot(self):
        with TempSubvolumes() as ts:
            sv1 = ts.create("test1")
            sv1.run_as_root(["touch", sv1.path("foo")])
            sv2 = ts.snapshot(sv1, "test2")
            sv1.run_as_root(["touch", sv1.path("bar")])
            sv2.run_as_root(["touch", sv2.path("baz")])
            self.assertTrue(os.path.exists(sv2.path("foo")))
            self.assertFalse(os.path.exists(sv2.path("bar")))
            self.assertFalse(os.path.exists(sv1.path("baz")))

    def test_temp_subvolumes_caller_will_create(self):
        with TempSubvolumes() as ts:
            sv_path = ts.temp_dir / "test"
            sv = ts.caller_will_create("test")
            self.assertEqual(sv._path, sv_path)
            # Path should not actually exist
            self.assertFalse(os.path.exists(sv_path))
            self.assertFalse(sv._exists)

    def test_temp_subvolumes_outside_volume(self):
        with TempSubvolumes() as ts:
            with self.assertRaises(AssertionError):
                sv_path = ts.create("../breaking/the/law")

    def test_find_subvolume_on_disk(self):
        self.assertTrue(
            find_subvolume_on_disk(
                os.path.join(os.path.dirname(__file__), "hello_world_base")
            )
            .subvolume_path()
            .exists()
        )

    def test_estimate_content_bytes(self):
        with TempSubvolumes() as ts:
            sv = ts.create("test1")
            # Write a file with random data.  53kb because the size doesn't
            # really matter and prime is the coolest.
            sv.overwrite_path_as_root(Path("data"), contents=os.urandom(53 * KiB))
            estimated_fs_bytes = sv.estimate_content_bytes()

            # This _should_ be 54272 to match the exact number of bytes
            # written to the file, but the way we calculate estimated size
            # with `du` is providing us with actual "disk usage", and not
            # the "apparent size" as provided by the `--apparent-size`
            # switch.  See `man du` for more details.
            n_bytes = 53 * KiB
            self.assertGreaterEqual(estimated_fs_bytes, n_bytes)
            self.assertLess(
                estimated_fs_bytes, n_bytes + 4096
            )  # 4K is the max reasonable block size?

    @with_temp_subvols
    def test_delete_error(self, ts: TempSubvolumes):
        sv = ts.create("test_delete_error")
        with unittest.mock.patch(
            "antlir.subvol_utils.btrfsutil.delete_subvolume",
            side_effect=BtrfsUtilError(errno.EINVAL, None),
        ):
            with self.assertRaises(BtrfsUtilError, msg="blah"):
                sv.delete()
        with unittest.mock.patch(
            "antlir.subvol_utils.btrfsutil.delete_subvolume",
            side_effect=BtrfsUtilError(errno.ENOENT, None),
        ):
            sv.delete()
