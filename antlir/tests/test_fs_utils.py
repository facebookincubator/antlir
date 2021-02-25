#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import ast
import errno
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
from io import StringIO

from ..common import byteme, check_popen_returncode
from ..fs_utils import (
    Path,
    create_ro,
    generate_work_dir,
    open_for_read_decompress,
    populate_temp_dir_and_rename,
    populate_temp_file_and_rename,
    temp_dir,
)


_BAD_UTF = b"\xc3("


class TestFsUtils(unittest.TestCase):
    def test_path_basics(self):
        self.assertEqual(
            byteme(os.getcwd()) + b"/foo/bar", Path("foo/bar").abspath()
        )
        self.assertEqual(b"/a/c", Path("/a/b/../c").realpath())
        self.assertEqual(b"foo/bar", Path("foo") / "bar")
        self.assertEqual(b"/foo/bar", b"/foo" / Path.or_none("bar"))
        self.assertEqual(b"/baz", b"/be/bop" / Path(b"/baz"))
        self.assertEqual("file:///a%2Cb", Path("/a,b").file_url())
        self.assertEqual(b"bom", Path("/bim/bom").basename())
        self.assertEqual(b"/bim", Path("/bim/bom").dirname())
        self.assertEqual(b"ta/da", Path("./ta//gr/../da/").normpath())
        self.assertEqual(b"/a/c", Path("/a/b/../c").realpath())
        self.assertEqual(b"../c/d/e", Path("/a/b/c/d/e").relpath("/a/b/x"))
        self.assertEqual(b"../../../y/z", Path("/y/z").relpath("/a/b/x"))
        self.assertEqual(Path("foo"), Path("foo"))
        self.assertIsNone(Path.or_none(None))
        with self.assertRaises(TypeError):
            Path("foo") == "foo"
        with self.assertRaises(TypeError):
            Path("foo") != "foo"
        with self.assertRaises(TypeError):
            Path("foo") > "foo"
        with self.assertRaises(TypeError):
            Path("foo") >= "foo"
        with self.assertRaises(TypeError):
            Path("foo") < "foo"
        with self.assertRaises(TypeError):
            Path("foo") <= "foo"

    def test_path_is_hashable(self):
        # Path must be hashable to be added to a set
        ts = set()
        ts.add(Path("foo"))

    def test_bad_utf_is_bad(self):
        with self.assertRaises(UnicodeDecodeError):
            _BAD_UTF.decode()

    def test_path_decode(self):
        with tempfile.TemporaryDirectory() as td:
            bad_utf_path = Path(td) / _BAD_UTF
            self.assertTrue(bad_utf_path.endswith(b"/" + _BAD_UTF))
            with open(bad_utf_path, "w"):
                pass
            res = subprocess.run(
                [
                    sys.executable,
                    "-c",
                    f"import os;print(os.listdir({repr(td)}))",
                ],
                stdout=subprocess.PIPE,
            )
            # Path's handling of invalid UTF-8 matches the default for
            # Python3 when it gets such data from the filesystem.
            self.assertEqual(
                # Both evaluate to surrogate-escaped ['\udcc3('] plus a newline.
                repr([bad_utf_path.basename().decode()]) + "\n",
                res.stdout.decode(),
            )

    def test_path_exists(self):
        does_not_exist = Path("non/existent")
        for err in [True, False]:
            self.assertFalse(does_not_exist.exists(raise_permission_error=err))

        with temp_dir() as td:
            i_exist = td / "cogito_ergo_sum"
            i_exist.touch()
            for err in [True, False]:
                self.assertTrue(i_exist.exists(raise_permission_error=err))

            if os.geteuid() == 0:
                return  # Skip "permission error" tests, `root` can see all.

            old_mode = os.stat(td).st_mode
            try:
                os.chmod(td, 0)
                self.assertFalse(i_exist.exists(raise_permission_error=False))
                with self.assertRaises(PermissionError):
                    i_exist.exists(raise_permission_error=True)
            finally:
                os.chmod(td, old_mode)

    def test_path_islink(self):
        with temp_dir() as td:
            target = td / "target"
            link = td / "link"

            # Real files aren't symlinks
            self.assertFalse(target.islink())

            os.symlink(target, link)

            # Broken symlinks are still symlinks
            self.assertTrue(link.islink())

            # Non-broken symlinks are symlinks :)
            target.touch()
            self.assertTrue(link.islink())

    def test_path_wait_for(self):
        with tempfile.TemporaryDirectory() as td:
            to_wait_for = Path(td) / "will_you_wait_for_me"

            def _make_file():
                to_wait_for.touch()

            t = threading.Timer(0.1, _make_file)
            t.start()

            # This will return without an exception
            elapsed_ms = to_wait_for.wait_for(timeout_ms=100000)
            self.assertTrue(elapsed_ms > 0)

            # Just to be sure
            t.cancel()

            # Reset the file to re-run the test for negative assertion
            os.unlink(to_wait_for)

            with self.assertRaises(FileNotFoundError):
                to_wait_for.wait_for(timeout_ms=100)

    def test_path_format(self):
        first = Path("a/b")
        second = Path(_BAD_UTF)
        formatted = "^a/b       >" + _BAD_UTF.decode(errors="surrogateescape")
        self.assertEqual(formatted, f"^{first:10}>{second}")

    def test_path_from_argparse(self):
        res = subprocess.run(
            [
                sys.executable,
                "-c",
                "import sys;print(repr(sys.argv[1]))",
                _BAD_UTF,
            ],
            stdout=subprocess.PIPE,
        )
        # Demangle non-UTF bytes in the same way that `sys.argv` mangles them.
        self.assertEqual(
            _BAD_UTF,
            Path.from_argparse(
                ast.literal_eval(res.stdout.rstrip(b"\n").decode())
            ),
        )

    def test_normalized_subpath(self):
        for p in [Path("/need/not/exist"), Path("something/relative")]:
            self.assertEqual(p, p.normalized_subpath("."))

            for bad_path in ["..", "a/../../b/c/d", "../c/d/e"]:
                with self.assertRaisesRegex(AssertionError, "is outside of"):
                    p.normalized_subpath(bad_path)

            self.assertEqual(
                p.normalized_subpath("a/b"), p.normalized_subpath("/a/b/.")
            )

            self.assertEqual(b"a/b", p.normalized_subpath("a/b").relpath(p))

    def test_path_json(self):
        # We can serialize `Path` to JSON, including invalid UTF-8.
        # Unfortunately, `json` doesn't allow us to custom-serialize keys.
        obj_in = {"a": Path("b"), "c": Path(_BAD_UTF), "builtin": 3}
        # Deserializing to `Path` requires the consumer to know the type
        # schema.
        obj_out = {
            "a": "b",
            "c": _BAD_UTF.decode(errors="surrogateescape"),
            "builtin": 3,
        }
        self.assertEqual(obj_out, json.loads(Path.json_dumps(obj_in)))
        f = StringIO()
        Path.json_dump(obj_in, f)
        f.seek(0)
        self.assertEqual(obj_out, json.load(f))
        with self.assertRaises(TypeError):
            Path.json_dumps({"not serializable": object()})

    def test_path_listdir(self):
        with temp_dir() as td:
            (td / "a").touch()
            (a,) = td.listdir()
            self.assertIsInstance(a, Path)
            self.assertEqual(b"a", a)

    def test_path_parse_args(self):
        p = argparse.ArgumentParser()
        p.add_argument("--path", action="append", type=Path.from_argparse)
        # Check that `Path` is now allowed, and that we can round-trip bad UTF.
        argv = ["--path", Path("a"), "--path", Path(_BAD_UTF)]
        with self.assertRaises(TypeError):
            p.parse_args(argv)
        args = Path.parse_args(p, argv)
        self.assertEqual([Path("a"), Path(_BAD_UTF)], args.path)

    def test_path_read_text(self):
        with temp_dir() as td:
            tmp_path = Path(td / "foo.txt")
            with open(tmp_path, "w+") as f:
                f.write("hello\n")
            self.assertEqual("hello\n", tmp_path.read_text())

    def test_path_open(self):
        with temp_dir() as td:
            tmp_path = Path(td / "foo.txt")
            with tmp_path.open(mode="w+") as f:
                f.write("hello\n")
            with tmp_path.open() as f:
                self.assertEqual("hello\n", f.read())

    def test_path_shell_quote(self):
        self.assertEqual(
            Path(r"""/a\ b/c d/e'"f/( \t/""").shell_quote(),
            r"""'/a\ b/c d/e'"'"'"f/( \t/'""",
        )

    def test_path_str(self):
        self.assertEqual("a/b", str(Path("a/b")))
        self.assertEqual(
            _BAD_UTF.decode(errors="surrogateescape"), str(Path(_BAD_UTF))
        )

    def test_path_has_leading_dot_dot(self):
        self.assertTrue(Path("..").has_leading_dot_dot())
        self.assertTrue(Path("../a/b/c").has_leading_dot_dot())
        self.assertFalse(Path("..a/b/c").has_leading_dot_dot())
        self.assertFalse(Path("a/../b/c").has_leading_dot_dot())
        # This shows that we don't normalize, thus this function does not
        # check whether the relative path refers outside of its base.
        self.assertFalse(Path("a/../../b/c").has_leading_dot_dot())

    def test_path_touch(self):
        with temp_dir() as td:
            tmp_path = td / "touchme"
            tmp_path.touch()

            self.assertTrue(os.path.exists(tmp_path))

    def test_path_validate(self):
        result = "a/b"
        for validator in Path.__get_validators__():
            result = validator(result)
        self.assertEqual(result, Path("a/b"))
        self.assertIsInstance(result, Path)

    def test_open_for_read_decompress(self):
        # The goal is that our stream should be bigger than any buffers
        # involved (so we get to test edge effects), but not so big that the
        # test takes more than 1-2 seconds.
        n_bytes = 12 << 20  # 12MiB
        my_line = b"kitteh" * 700 + b"\n"  # ~ 4KiB
        for compress, ext in [("gzip", "gz"), ("zstd", "zst")]:
            filename = "kitteh." + ext
            with temp_dir() as td, open(td / filename, "wb") as outf:
                with subprocess.Popen(
                    [compress, "-"], stdin=subprocess.PIPE, stdout=outf
                ) as proc:
                    for _ in range(n_bytes // len(my_line)):
                        proc.stdin.write(my_line)
                check_popen_returncode(proc)

                with open_for_read_decompress(td / filename) as infile:
                    for l in infile:
                        self.assertEqual(my_line, l)

                # Test that an incomplete read doesn't cause SIGPIPE
                with open_for_read_decompress(td / filename) as infile:
                    pass

        # Test uncompressed
        with temp_dir() as td:
            with open(td / "kitteh", "wb") as outfile:
                outfile.write(my_line + b"meow")
            with open_for_read_decompress(td / "kitteh") as infile:
                self.assertEqual(my_line + b"meow", infile.read())

        # Test decompression error
        with temp_dir() as td:
            with open(td / "kitteh.gz", "wb") as outfile:
                outfile.write(my_line)
            with self.assertRaises(
                subprocess.CalledProcessError
            ), open_for_read_decompress(td / "kitteh.gz") as infile:
                infile.read()

    def test_create_ro(self):
        with temp_dir() as td:
            with create_ro(td / "hello_ro", "w") as out_f:
                out_f.write("world_ro")
            with open(td / "hello_rw", "w") as out_f:
                out_f.write("world_rw")

            # `_create_ro` refuses to overwrite both RO and RW files.
            with self.assertRaises(FileExistsError):
                create_ro(td / "hello_ro", "w")
            with self.assertRaises(FileExistsError):
                create_ro(td / "hello_rw", "w")

            # Regular `open` can accidentelly clobber the RW, but not the RW.
            if os.geteuid() != 0:  # Root can clobber anything :/
                with self.assertRaises(PermissionError):
                    open(td / "hello_ro", "a")
            with open(td / "hello_rw", "a") as out_f:
                out_f.write(" -- appended")

            with open(td / "hello_ro") as in_f:
                self.assertEqual("world_ro", in_f.read())
            with open(td / "hello_rw") as in_f:
                self.assertEqual("world_rw -- appended", in_f.read())

    def _check_has_one_file(self, dir_path, filename, contents):
        self.assertEqual([filename.encode()], os.listdir(dir_path))
        with open(dir_path / filename) as in_f:
            self.assertEqual(contents, in_f.read())

    def test_populate_temp_dir_and_rename(self):
        with temp_dir() as td:
            # Create and populate "foo"
            foo_path = td / "foo"
            with populate_temp_dir_and_rename(foo_path) as td2:
                self.assertTrue(td2.startswith(td + b"/"))
                self.assertEqual(td2, td / td2.basename())
                self.assertNotEqual(td2.basename(), Path("foo"))
                with create_ro(td2 / "hello", "w") as out_f:
                    out_f.write("world")
            self._check_has_one_file(foo_path, "hello", "world")

            # Fail to overwrite
            with self.assertRaises(OSError) as ctx:
                with populate_temp_dir_and_rename(foo_path):
                    pass  # Try to overwrite with empty.
            # Different kernels return different error codes :/
            self.assertIn(ctx.exception.errno, [errno.ENOTEMPTY, errno.EEXIST])
            self._check_has_one_file(foo_path, "hello", "world")  # No change

            # Force-overwrite
            with populate_temp_dir_and_rename(foo_path, overwrite=True) as td2:
                with create_ro(td2 / "farewell", "w") as out_f:
                    out_f.write("arms")
            self._check_has_one_file(foo_path, "farewell", "arms")

    def test_populate_temp_file_and_rename_success(self):
        with temp_dir() as td:
            path = td / "dog"
            with populate_temp_file_and_rename(path) as outfile:
                outfile.write("woof")
                tmp_path = outfile.name
            # Temp file should be deleted
            self.assertFalse(os.path.exists(tmp_path))
            # Ensure that file exists and contains correct content
            self.assertTrue(os.path.exists(path))
            self.assertEqual(path.read_text(), "woof")

    def test_populate_temp_file_fail_to_overwrite(self):
        with temp_dir() as td:
            path = td / "dog"
            with open(path, "w") as outfile:
                outfile.write("woof")
            # Fail to write due to existing file
            with self.assertRaises(FileExistsError):
                with populate_temp_file_and_rename(path) as outfile:
                    outfile.write("meow")
                    tmp_path = outfile.name
            # Temp file should be deleted
            self.assertFalse(os.path.exists(tmp_path))
            # Original file is untouched
            self.assertEqual(path.read_text(), "woof")

    def test_populate_temp_file_force_overwrite(self):
        with temp_dir() as td:
            path = td / "dog"
            with open(path, "w") as outfile:
                outfile.write("woof")
            # Succeeds in overwriting contents in "dog"
            with populate_temp_file_and_rename(path, overwrite=True) as outfile:
                outfile.write("meow")
                tmp_path = outfile.name
            # Temp file should no longer exist (as it has been renamed)
            self.assertFalse(os.path.exists(tmp_path))
            # Original file is modified
            self.assertEqual(path.read_text(), "meow")

    def test_populate_temp_file_and_rename_error(self):
        with temp_dir() as td:
            path = td / "dog"
            with open(path, "w") as outfile:
                outfile.write("woof")
            with self.assertRaisesRegex(RuntimeError, "^woops$"):
                with populate_temp_file_and_rename(path) as outfile:
                    outfile.write("meow")
                    tmp_path = outfile.name
                    raise RuntimeError("woops")
            # Temp file should be deleted
            self.assertFalse(os.path.exists(tmp_path))
            # the original file is untouched
            self.assertEqual(path.read_text(), "woof")

    def test_generate_work_dir(self):
        work_dir = generate_work_dir()

        # make sure we stripped the = padding out
        self.assertNotIn("=", work_dir)

        # A b64 encoded uuid is 22 chars. That plus the
        # '/work' prefix is 27 chars,
        self.assertTrue(len(work_dir) == 27)

    def test_strip_leading_slashes(self):
        for p, want in (
            ("", ""),
            ("/", ""),
            ("//", ""),
            ("///", ""),
            ("/a/b/c", "a/b/c"),
            ("//d/e", "d/e"),
        ):
            self.assertEqual(Path(p).strip_leading_slashes(), Path(want))
