#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import re
import subprocess
import sys
import tempfile
import unittest.mock

from antlir.btrfs_diff.tests.demo_sendstreams_expected import (
    render_demo_as_corrupted_by_gnu_tar,
    render_demo_subvols,
)

from ..artifacts_dir import ensure_per_repo_artifacts_dir_exists
from ..fs_utils import Path, temp_dir
from ..loopback_opts_t import loopback_opts_t
from ..subvol_utils import (
    find_subvolume_on_disk,
    MiB,
    Subvol,
    TempSubvolumes,
    volume_dir,
    with_temp_subvols,
    _query_uuid,
    _UUID_TO_SUBVOLS,
)
from ..volume_for_repo import get_volume_for_current_repo
from .common import AntlirTestCase
from .subvol_helpers import render_subvol


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
        self.assertTrue(
            re.match("[a-f0-f]{32}$", subvol._uuid.replace("-", "")),
            subvol._uuid,
        )
        self.assertTrue(subvol.path().exists(), subvol.path())

    def _assert_subvol_does_not_exist(self, subvol):
        self.assertEqual((False, None), (subvol._exists, subvol._uuid))
        self.assertFalse(subvol.path().exists(), subvol.path())

    # Checks that `_mark_{created,deleted}` work as expected, including
    # nested subvolumes, read-only subvolumes, and `Subvol`s that are clones
    # of one another.
    @with_temp_subvols
    def test_create_and_delete_on_exit(self, temp_subvols):
        def subvol_stats():
            return (
                sum(len(subvols) for subvols in _UUID_TO_SUBVOLS.values()),
                len(_UUID_TO_SUBVOLS),
            )

        subvol_count, uuid_count = subvol_stats()

        s1 = temp_subvols.caller_will_create("create_and_delete")
        self._assert_subvol_does_not_exist(s1)
        self.assertEqual((subvol_count, uuid_count), subvol_stats())

        with s1.create().delete_on_exit():
            self._assert_subvol_exists(s1)
            self.assertEqual((subvol_count + 1, uuid_count + 1), subvol_stats())

            with Subvol(s1.path(), already_exists=True).delete_on_exit() as s2:
                self._assert_subvol_exists(s2)
                self._assert_subvol_exists(s2)
                self.assertEqual(
                    (subvol_count + 2, uuid_count + 1), subvol_stats()
                )

                nested = Subvol(s1.path("nested")).create()
                # Cover the "nested untracked subvol" case -- this subvol
                # will not be in `_UUID_TO_SUBVOLS`.  This deliberately
                # omits `Subvol.maybe_create_externally()`.
                s1.run_as_root(
                    ["btrfs", "subvolume", "create", s1.path("nested_extern")],
                )

                self._assert_subvol_exists(nested)
                self.assertTrue(os.path.exists(s1.path("nested_extern")))
                self.assertEqual(
                    # This is not counting "nested_extern".
                    (subvol_count + 3, uuid_count + 2),
                    subvol_stats(),
                )

                # Verify that parents get marked RW before deleting children.
                s2.set_readonly(True)  # is a clone of `s1`
                # It's OK if a child is read-only, too.
                nested.set_readonly(True)

            self._assert_subvol_does_not_exist(s1)
            self._assert_subvol_does_not_exist(s2)
            self._assert_subvol_does_not_exist(nested)
            self.assertEqual((subvol_count, uuid_count), subvol_stats())

        self.assertEqual((subvol_count, uuid_count), subvol_stats())

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
                self.assertFalse(sv._exists)
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
                unittest.mock.Mock(side_effect=[True]),
            ), unittest.mock.patch(
                "antlir.subvol_utils._query_uuid",
                unittest.mock.Mock(side_effect=["FAKE-UUID-000"]),
            ):
                sv = Subvol(td, already_exists=True)
            os.mkdir(td / "real")
            (td / "real/file").touch()
            os.symlink("real/file", td / "indirect1")
            os.mkdir(td / "indirect2")
            os.symlink("../indirect1", td / "indirect2/link")
            self.assertEqual(
                b"/real/file", sv.canonicalize_path("indirect2/link")
            )
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
    def _test_mark_readonly_and_send_to_new_loopback(
        self, temp_subvols, multi_pass_size_minimization
    ):
        loopback_opts = loopback_opts_t(
            minimize_size=multi_pass_size_minimization,
            subvol_name="check_subvol_rename",
        )
        sv = temp_subvols.create("subvol")
        sv.run_as_root(
            [
                "dd",
                "if=/dev/urandom",
                b"of=" + sv.path("d"),
                "bs=1M",
                "count=600",
            ]
        )
        sv.run_as_root(["mkdir", sv.path("0")])
        sv.run_as_root(["tee", sv.path("0/0")], input=b"0123456789")

        # The default waste factor succeeds in 1 try, but a too-low
        # factor results in 2 tries.
        waste_too_low = 1.00015

        with tempfile.NamedTemporaryFile() as loop_path:
            self.assertEqual(
                2,
                sv.mark_readonly_and_send_to_new_loopback(
                    loop_path.name,
                    loopback_opts=loopback_opts,
                    waste_factor=waste_too_low,
                ),
            )

            # Now do it again without the waste factor so that
            # it only takes one pass
            self.assertEqual(
                1,
                sv.mark_readonly_and_send_to_new_loopback(
                    loop_path.name, loopback_opts=loopback_opts
                ),
            )

            # Same 2-try run, but this time, exercise the free space check
            # instead of relying on parsing `btrfs receive` output.
            with unittest.mock.patch.object(
                Subvol, "_OUT_OF_SPACE_SUFFIX", b"cypa"
            ):
                self.assertEqual(
                    1,
                    sv.mark_readonly_and_send_to_new_loopback(
                        loop_path.name,
                        loopback_opts=loopback_opts,
                    ),
                )

    def test_mark_readonly_and_send_to_new_loopback(self):
        self._test_mark_readonly_and_send_to_new_loopback(
            multi_pass_size_minimization=False
        )

    def test_mark_readonly_and_send_to_new_loopback_with_multi_pass(self):
        self._test_mark_readonly_and_send_to_new_loopback(
            multi_pass_size_minimization=True
        )

    @with_temp_subvols
    def test_mark_readonly_and_send_to_new_loopback_writable(
        self, temp_subvols
    ):
        # `test_package_image_as_btrfs_loopback_writable` actually
        # tests that the subvolume is writable, here we just test that
        # the subvol util helper method works
        sv = temp_subvols.create("subvol")
        sv.run_as_root(
            ["dd", "if=/dev/zero", b"of=" + sv.path("d"), "bs=1M", "count=200"]
        )
        sv.run_as_root(["mkdir", sv.path("0")])
        sv.run_as_root(["tee", sv.path("0/0")], input=b"0123456789")
        with tempfile.NamedTemporaryFile() as loop_path:
            self.assertEqual(
                1,
                sv.mark_readonly_and_send_to_new_loopback(
                    loop_path.name,
                    loopback_opts=loopback_opts_t(writeable_subvolume=True),
                ),
            )

    @with_temp_subvols
    def test_mark_readonly_and_send_to_new_loopback_seed_device(
        self, temp_subvols
    ):
        # `test_package_image_as_btrfs_seed_device` actually
        # tests that the resulting image has the SEEDING flag set, here we just
        # test that the subvol util helper method works
        sv = temp_subvols.create("subvol")
        sv.run_as_root(
            ["dd", "if=/dev/zero", b"of=" + sv.path("d"), "bs=1M", "count=200"]
        )
        sv.run_as_root(["mkdir", sv.path("0")])
        sv.run_as_root(["tee", sv.path("0/0")], input=b"0123456789")
        with tempfile.NamedTemporaryFile() as loop_path:
            self.assertEqual(
                1,
                sv.mark_readonly_and_send_to_new_loopback(
                    loop_path.name,
                    loopback_opts=loopback_opts_t(
                        writable_subvolume=True, seed_device=True
                    ),
                ),
            )

    @with_temp_subvols
    def test_mark_readonly_and_send_to_new_loopback_explicit_size(
        self, temp_subvols
    ):
        sv = temp_subvols.create("subvol")
        sv.run_as_root(
            ["dd", "if=/dev/zero", b"of=" + sv.path("d"), "bs=1M", "count=200"]
        )
        sv.run_as_root(["mkdir", sv.path("0")])
        sv.run_as_root(["tee", sv.path("0/0")], input=b"0123456789")
        with tempfile.NamedTemporaryFile() as loop_path:
            self.assertEqual(
                1,
                sv.mark_readonly_and_send_to_new_loopback(
                    loop_path.name,
                    loopback_opts=loopback_opts_t(
                        # Make this size slightly larger than the subvol
                        size_mb=225,
                    ),
                ),
            )

            self.assertEqual(
                os.stat(loop_path.name).st_size,
                225 * MiB,
            )

    @with_temp_subvols
    def test_mark_readonly_and_send_to_new_loopback_default_subvol(
        self, temp_subvols
    ):
        sv = temp_subvols.create("subvol")
        sv.run_as_root(
            ["dd", "if=/dev/zero", b"of=" + sv.path("d"), "bs=1M", "count=200"]
        )
        sv.run_as_root(["mkdir", sv.path("0")])
        sv.run_as_root(["tee", sv.path("0/0")], input=b"0123456789")
        with tempfile.NamedTemporaryFile() as loop_path:
            self.assertEqual(
                1,
                sv.mark_readonly_and_send_to_new_loopback(
                    loop_path.name,
                    loopback_opts=loopback_opts_t(
                        default_subvolume=True,
                    ),
                ),
            )

    @with_temp_subvols
    def test_receive(self, temp_subvols):
        new_subvol_name = "differs_from_create_ops"
        sv = temp_subvols.caller_will_create(new_subvol_name)
        with open(
            Path(__file__).dirname() / "create_ops.sendstream"
        ) as f, sv.receive(f):
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

        demo_render = render_demo_as_corrupted_by_gnu_tar(
            create_ops=demo_sv_name
        )

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
    def test_read_file_as_root(self, ts: TempSubvolumes):
        rootfile = "testroot"
        contents = "0123456789"
        sv = ts.create("subvol")
        sv.overwrite_path_as_root(Path(rootfile), contents=contents)
        # Should be able to read now
        sv.run_as_root(["chmod", "0444", sv.path(rootfile)])
        self.assertEqual(sv.read_path_text(Path(rootfile)), contents)
        sv.run_as_root(["chmod", "0000", sv.path(rootfile)])
        # Should still be able to read it even though 0000
        self.assertEqual(sv.read_path_text_as_root(Path(rootfile)), contents)
        # Confirm that we fail if the file isn't there
        with self.assertRaises(FileNotFoundError):
            sv.read_path_text_as_root(Path("/nonsense"))

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

    def test_query_uuid(self):
        stdout = (
            b"\n\tName: \t\t\tvolume\n\t"
            b"UUID: \t\t\t8ec28ee3-e2cf-3345-8871-4bc4f85a3efc\n"
        )
        with unittest.mock.patch(
            "antlir.subvol_utils.Subvol.run_as_root",
            unittest.mock.Mock(
                return_value=subprocess.CompletedProcess(
                    args=[],
                    returncode=0,
                    stdout=stdout,
                )
            ),
        ):
            self.assertEqual(
                _query_uuid(
                    Subvol("/dev/null/unused"), "/dev/null/no-such-dir"
                ),
                "8ec28ee3-e2cf-3345-8871-4bc4f85a3efc",
            )
