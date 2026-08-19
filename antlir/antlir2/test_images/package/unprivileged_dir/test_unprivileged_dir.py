# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# pyre-strict

import os
import os.path
import stat
from pathlib import Path

from later.unittest import TestCase


class TestUnprivilegedDir(TestCase):
    def setUp(self) -> None:
        self.maxDiff = None

    def test_standard(self) -> None:
        # python_unittest resources are often (but not always and not entirely)
        # packaged up as a symlink tree. This makes looking at the actual
        # metadata of the underlying dir really hard...
        # So, we can compromise and still write a useful test by ensuring:
        # 1) all file/dir names we expect do exist
        # 2) an executable file is executable
        # 3) a symlink has the right target
        # 4) a file has the correct contents
        # 5) a large file has the correct number of bytes
        # 6) all files are owned by the unprivileged user
        path = Path("/unprivileged_dir")
        uid = os.getuid()
        gid = os.getgid()

        root = Path(os.path.realpath(path))
        files = set()
        dirs = set()
        stats = {}
        for dirpath, dirnames, filenames in root.walk():
            for dirname in dirnames:
                item = dirpath / dirname
                dirs.add(str(item.relative_to(root)))
                try:
                    stat_info = item.stat()
                except FileNotFoundError:
                    stat_info = item.lstat()
                stats[str(item.relative_to(root))] = stat_info
            for filename in filenames:
                item = dirpath / filename
                files.add(str(item.relative_to(root)))
                try:
                    stat_info = item.stat()
                except FileNotFoundError:
                    stat_info = item.lstat()
                stats[str(item.relative_to(root))] = stat_info

        # 1) all file/dir names we expect do exist
        self.assertEqual(
            {
                ".meta",
                "default-dir",
                "dir-with-xattrs",
                "hardlink",
                "identical",
                "sticky-dir",
            },
            dirs,
        )
        self.assertEqual(
            {
                ".meta/target",
                "absolute-dir-symlink",
                "absolute-file-symlink",
                "antlir2-large-file-256M",
                "default-dir/executable",
                "default-dir/relative-file-symlink",
                "hardlink/aloha",
                "hardlink/hello",
                "i-am-owned-by-nonstandard",
                "i-have-caps",
                "i-have-xattrs",
                "identical/file-1",
                "identical/file-2",
                "only-readable-by-root",
                "relative-dir-symlink",
                "setgid-file",
                "setuid-file",
            },
            files,
        )

        # 2) an executable file is executable
        # Only the executable bit survives a round trip through buck2's CAS, so
        # the rest of the permission bits are not stable enough to assert on
        # (they depend on whether the artifact was built locally or downloaded).
        executable_mode = stat.S_IMODE(stats["default-dir/executable"].st_mode)
        self.assertEqual(
            0o111,
            executable_mode & 0o111,
            f"executable is not actually executable: {executable_mode:o}",
        )
        self.assertEqual(
            0o444,
            executable_mode & 0o444,
            f"executable is not readable: {executable_mode:o}",
        )

        # 3) a symlink has the right target
        try:
            target = (root / "absolute-file-symlink").resolve()
            self.assertEqual("/default-dir/executable", str(target))
        except FileNotFoundError as e:
            # packaging issues are weird...
            self.assertEqual("/default-dir/executable", e.filename)

        # I can't figure out a sane way to check a relative symlink here...

        # 4) a file has the correct contents
        with open(root / "hardlink/hello") as f:
            self.assertEqual("Hello world\n", f.read())

        # 5) a large file has the correct number of bytes
        self.assertEqual(
            268435513,
            stats["antlir2-large-file-256M"].st_size,
            "large file was not copied fully",
        )

        # 6) all files are owned by the unprivileged user
        for relpath, stat_info in stats.items():
            self.assertEqual(uid, stat_info.st_uid, f"{relpath} owned by wrong user")
            self.assertEqual(gid, stat_info.st_gid, f"{relpath} owned by wrong group")

        # Note: We can't currently verify hardlinks because feature.install uses
        # copy_with_metadata, which doesn't preserve them
