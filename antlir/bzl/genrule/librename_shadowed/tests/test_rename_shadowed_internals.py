#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import ctypes
import ctypes.util
import os
import tempfile
import unittest
from contextlib import contextmanager

from antlir.fs_utils import create_ro, Path, temp_dir


class _Namespace:
    pass


@contextmanager
def _capture_fd(fd: int, *, inheritable: bool = True):
    with tempfile.TemporaryFile() as tf_out:
        fd_backup = os.dup(fd)
        try:
            os.dup2(tf_out.fileno(), fd, inheritable=inheritable)
            res = _Namespace()
            yield res
            tf_out.seek(0)
            # pyre-fixme[16]: `_Namespace` has no attribute `contents`.
            res.contents = tf_out.read()
        finally:
            os.dup2(fd_backup, fd, inheritable=inheritable)


# Tests C code from Python because this avoids taking on a dependency on
# gtest (and a custom pure-C test would have much worse CI integration.)
#
# Note: we gleefully leak the returned pointers here. #ramischeap
class TestRenameShadowedInternals(unittest.TestCase):
    @classmethod
    def addClassCleanup(cls, func, *args, **kwargs) -> None:
        cls._fakeClassCleanup = []
        # If we're not on 3.8, use our own, leakier cleanup strategy.  Test
        # cleanup methods leak on `SystemExit` et al anyway, so **shrug**.
        if hasattr(unittest.TestCase, "addClassCleanup"):  # Test in P130591520
            super().addClassCleanup(func, *args, **kwargs)
        else:
            cls._fakeClassCleanup.append((func, args, kwargs))

    @classmethod
    def tearDownClass(cls) -> None:
        for func, args, kwargs in cls._fakeClassCleanup:
            func(*args, **kwargs)

    # This has to be class-level, with `cls._shadow` shared among test
    # cases, because we can only load (and configure) the library once.
    @classmethod
    def setUpClass(cls) -> None:
        td_ctx = temp_dir()
        cls._shadow = td_ctx.__enter__()
        # NB: This may leak on SystemExit et al
        cls.addClassCleanup(td_ctx.__exit__, None, None, None)

        os.environ["ANTLIR_SHADOWED_PATHS_ROOT"] = f"{cls._shadow}"

        lib_ctx = Path.resource(__package__, "librename_shadowed.so", exe=False)
        lib_path = lib_ctx.__enter__()
        # NB: This may leak a tempfile on SystemExit et al
        cls.addClassCleanup(lib_ctx.__exit__, None, None, None)

        # pyre-fixme[6]: For 1st param expected `str` but got `Path`.
        lib = ctypes.cdll.LoadLibrary(lib_path)

        cls._get_shadowed_original = lib.get_shadowed_original
        cls._get_shadowed_original.restype = ctypes.c_char_p
        cls._get_shadowed_original.argtypes = [ctypes.c_char_p]

        cls._get_shadowed_rename_dest = lib.get_shadowed_rename_dest
        cls._get_shadowed_rename_dest.restype = ctypes.c_char_p
        cls._get_shadowed_rename_dest.argtypes = [
            ctypes.c_char_p,
            ctypes.c_char_p,
        ]

        cls._rename = lib.rename
        cls._rename.restype = ctypes.c_int
        cls._rename.argtypes = [ctypes.c_char_p, ctypes.c_char_p]

    def test_get_shadowed_original(self) -> None:
        self.assertEqual(
            self._shadow / "etc/slon",
            self._get_shadowed_original(b"/etc//systemd/../slon"),
        )
        self.assertEqual(
            self._shadow / "borsch", self._get_shadowed_original(b"/./borsch")
        )
        # Make sure we handle relative paths, both empty & non-empty dirname
        os.chdir("/etc")
        self.assertEqual(
            self._shadow / "etc/kitteh", self._get_shadowed_original(b"kitteh")
        )
        self.assertEqual(
            self._shadow / "etc/kitteh",
            self._get_shadowed_original(b"./kitteh"),
        )
        self.assertEqual(
            self._shadow / "etc/systemd/maow",
            self._get_shadowed_original(b"systemd/maow"),
        )
        # This case doesn't come up in practice since we don't support
        # directories, but we might as well behave (somewhat) sanely.
        self.assertEqual(self._shadow + b"/", self._get_shadowed_original(b"/"))
        # The only error case we can reasonably check
        self.assertEqual(
            None, self._get_shadowed_original(b"/dir_that_does_not_exist/foo")
        )

    def test_get_shadowed_rename_dest(self) -> None:
        with temp_dir() as td:
            shadow_td = self._shadow / td.strip_leading_slashes()
            os.makedirs(shadow_td)

            # These `real_*` things have no shadow counterparts.
            os.mkdir(td / "real_dir_exists")
            with create_ro(td / "real_file_exists", "w"):
                pass

            # The shadow setup is OK for this one, but the source better not
            # be `real_file_exists`.
            os.link(td / "real_file_exists", td / "hardlink")
            with create_ro(shadow_td / "hardlink", "w"):
                pass

            # Good case: both exist and are files.
            for d in [td, shadow_td]:
                with create_ro(d / "shadow_and_real_exist", "w"):
                    pass

            # Destination is OK, but shadow is a directory (bug?)
            with create_ro(td / "real_file_shadow_dir", "w"):
                pass
            os.mkdir(shadow_td / "real_file_shadow_dir")

            # Destination does not exist
            self.assertEqual(
                None,
                self._get_shadowed_rename_dest(
                    td / "real_file_exists", td / "file_does_not_exist"
                ),
            )
            # Destination is a directory
            self.assertEqual(
                None,
                self._get_shadowed_rename_dest(b"/etc", td / "real_file_exists"),
            )
            # Same inode
            self.assertEqual(
                None,
                self._get_shadowed_rename_dest(
                    td / "real_file_exists", td / "hardlink"
                ),
            )
            # 'hardlink' destination is fine if not renaming the same inode
            self.assertEqual(
                shadow_td / "hardlink",
                self._get_shadowed_rename_dest(
                    td / "shadow_and_real_exist", td / "hardlink"
                ),
            )
            # Shadow destination does not exist
            self.assertEqual(
                None,
                self._get_shadowed_rename_dest(
                    b"/etc/shadow_and_real_exist", b"/etc/real_file_exists"
                ),
            )
            # Shadow destination is a directory
            self.assertEqual(
                None,
                self._get_shadowed_rename_dest(
                    b"/etc/real_file_exists", b"/etc/real_file_shadow_dir"
                ),
            )
            # Good case: different indoes, both destinations are files
            self.assertEqual(
                shadow_td / "shadow_and_real_exist",
                self._get_shadowed_rename_dest(
                    # We don't error-check the source being a directory,
                    # since `rename` will fail
                    td / "real_dir_exists",
                    td / "shadow_and_real_exist",
                ),
            )

    def _check_file_contents(self, file_contents) -> None:
        for f, c in file_contents:
            with open(f, "r") as f:
                self.assertEqual(c, f.read())

    def test_interposed_rename(self) -> None:
        with temp_dir() as td:
            shadow_td = self._shadow / td.strip_leading_slashes()
            os.makedirs(shadow_td)

            # Good case: a file gets renamed

            with create_ro(td / "gets_moved", "w") as f:
                f.write("i become shadow")
            for d in [td, shadow_td]:
                with create_ro(d / "ok_dest", "w") as f:
                    f.write("unmodified")

            self._check_file_contents(
                [
                    (td / "gets_moved", "i become shadow"),
                    (td / "ok_dest", "unmodified"),
                    (shadow_td / "ok_dest", "unmodified"),
                ]
            )

            with _capture_fd(2) as res:
                self.assertEqual(0, self._rename(td / "gets_moved", td / "ok_dest"))
            self.assertEqual(
                f"`rename({td}/gets_moved, {td}/ok_dest)` will replace "
                + f"shadowed original `{shadow_td}/ok_dest`\n",
                res.contents.decode(),
            )

            self.assertFalse(os.path.exists(td / "gets_moved"))
            self._check_file_contents(
                [
                    (td / "ok_dest", "unmodified"),
                    (shadow_td / "ok_dest", "i become shadow"),
                ]
            )

            # Normal case: destination lacks a shadow counterpart

            with create_ro(td / "also_moved", "w") as f:
                f.write("no shadow for me")
            with create_ro(td / "unshadowed", "w") as f:
                f.write("unmodified")

            self._check_file_contents(
                [
                    (td / "also_moved", "no shadow for me"),
                    (td / "unshadowed", "unmodified"),
                ]
            )

            with _capture_fd(2) as res:
                self.assertEqual(0, self._rename(td / "also_moved", td / "unshadowed"))
            self.assertEqual(b"", res.contents)

            self.assertFalse(os.path.exists(td / "also_moved"))
            self._check_file_contents([(td / "unshadowed", "no shadow for me")])
