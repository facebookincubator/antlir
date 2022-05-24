#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import contextlib
import fcntl
import os
import tempfile
import unittest

from .. import subvolume_garbage_collector as sgc
from ..fs_utils import Path, temp_dir
from ..subvol_utils import TempSubvolumes, with_temp_subvols


class SubvolumeGarbageCollectorTestCase(unittest.TestCase):
    # pyre-fixme[2]: Parameter must be annotated.
    def _touch(self, *path) -> None:
        with open(os.path.join(*path), "a"):
            pass

    def test_list_subvolume_wrappers(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            self.assertEqual([], sgc.list_subvolume_wrappers(tdp))

            self._touch(Path(tdp / "ba:nana"))  # Not a directory
            self.assertEqual([], sgc.list_subvolume_wrappers(tdp))

            os.mkdir(Path(tdp / "apple"))  # No colon
            self.assertEqual([], sgc.list_subvolume_wrappers(tdp))

            os.mkdir(Path(tdp / "p:i"))
            os.mkdir(Path(tdp / "e:"))
            os.mkdir(Path(tdp / ":x"))
            self.assertEqual(
                {Path("p:i"), Path("e:"), Path(":x")},
                set(sgc.list_subvolume_wrappers(tdp)),
            )

    def test_list_refcounts(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            # pyre-fixme[6]: For 1st param expected `Path` but got `str`.
            self.assertEqual({}, dict(sgc.list_refcounts(td)))

            self._touch(td, "foo:bar")  # No .json
            self._touch(td, "borf.json")  # No :
            # pyre-fixme[6]: For 1st param expected `Path` but got `str`.
            self.assertEqual({}, dict(sgc.list_refcounts(td)))

            banana_json = Path(td) / "ba:nana.json"
            os.mkdir(banana_json)  # Not a file
            with self.assertRaisesRegex(RuntimeError, "not a regular file"):
                # pyre-fixme[6]: For 1st param expected `Path` but got `str`.
                dict(sgc.list_refcounts(td))
            os.rmdir(banana_json)

            self._touch(banana_json)  # This is a real refcount file now
            # pyre-fixme[6]: For 1st param expected `Path` but got `str`.
            self.assertEqual({Path("ba:nana"): 1}, dict(sgc.list_refcounts(td)))

            # The linking is pathological, but it doesn't seem worth detecting.
            os.link(banana_json, Path(td) / "ap:ple.json")
            self.assertEqual(
                {Path("ba:nana"): 2, Path("ap:ple"): 2},
                # pyre-fixme[6]: For 1st param expected `Path` but got `str`.
                dict(sgc.list_refcounts(td)),
            )

            os.unlink(banana_json)
            # pyre-fixme[6]: For 1st param expected `Path` but got `str`.
            self.assertEqual({Path("ap:ple"): 1}, dict(sgc.list_refcounts(td)))

    # Not bothering with a direct test for `parse_args` because (a) it is
    # entirely argparse declarations, and that module has decent validation,
    # (b) we test it indirectly in `test_has_new_subvolume` and others.

    def test_has_new_subvolume(self) -> None:

        # Instead of creating a fake namespace, actually parse some args
        # pyre-fixme[3]: Return type must be annotated.
        # pyre-fixme[2]: Parameter must be annotated.
        def dir_json(wrapper_dir, json):
            args = ["--refcounts-dir=fake", "--subvolumes-dir=fake"]
            if wrapper_dir is not None:
                args.append(f"--new-subvolume-wrapper-dir={wrapper_dir}")
            if json is not None:
                args.append(f"--new-subvolume-json={json}")
            return sgc.parse_args(args)

        self.assertFalse(sgc.has_new_subvolume(dir_json(None, None)))
        self.assertTrue(sgc.has_new_subvolume(dir_json("x:y", "z")))

        for bad_example in [("x:y", None), (None, "z")]:
            with self.assertRaisesRegex(
                RuntimeError, "pass both .* or pass none"
            ):
                sgc.has_new_subvolume(dir_json(*bad_example))

        for bad_example in [("x/y", "z"), ("no_colon", "z")]:
            with self.assertRaisesRegex(
                RuntimeError, "must contain : but not /"
            ):
                sgc.has_new_subvolume(dir_json(*bad_example))

        with tempfile.TemporaryDirectory() as td:
            os.mkdir(Path(td) / "x:y")
            with self.assertRaisesRegex(RuntimeError, "wrapper-dir exists"):
                sgc.has_new_subvolume(
                    sgc.parse_args(
                        [
                            "--refcounts-dir=fake",
                            f"--subvolumes-dir={td}",
                            "--new-subvolume-wrapper-dir=x:y",
                            "--new-subvolume-json=fake",
                        ]
                    )
                )

    def test_gc_fails_when_wrapper_has_more_than_one(self) -> None:
        with tempfile.TemporaryDirectory() as refs_dir, tempfile.TemporaryDirectory() as subs_dir:  # noqa: E501
            os.makedirs(Path(subs_dir) / "no:refs/subvol1")
            os.makedirs(Path(subs_dir) / "no:refs/subvol2")
            with self.assertRaisesRegex(
                RuntimeError, "must contain just 1 subvol"
            ):
                sgc.subvolume_garbage_collector(
                    [
                        f"--refcounts-dir={refs_dir}",
                        f"--subvolumes-dir={subs_dir}",
                    ]
                )

    @with_temp_subvols
    # pyre-fixme[2]: Parameter must be annotated.
    def test_gc_clean_nspawn_lockfile(self, tmp_subvols) -> None:
        with temp_dir() as refs_dir:
            subs_dir = tmp_subvols.temp_dir
            os.makedirs(subs_dir / "no:refs")
            tmp_subvols.create("no:refs/subvol")
            (subs_dir / "no:refs/.#subvol.lck").touch()
            self.assertEqual([b"no:refs"], subs_dir.listdir())
            sgc.subvolume_garbage_collector(
                [
                    f"--refcounts-dir={refs_dir}",
                    f"--subvolumes-dir={subs_dir}",
                ]
            )
            self.assertEqual([], subs_dir.listdir())
            self.assertEqual([], refs_dir.listdir())

    @contextlib.contextmanager
    # pyre-fixme[3]: Return type must be annotated.
    def _gc_test_case(self):
        # NB: I'm too lazy to test that `refs_dir` is created if missing.
        with TempSubvolumes() as tmp_subvols, temp_dir() as refs_dir:
            subs_dir = tmp_subvols.temp_dir
            # Track subvolumes + refcounts that will get garbage-collected
            # separately from those that won't.
            gcd_subs = set()
            kept_subs = set()
            gcd_refs = set()
            kept_refs = set()

            # Subvolume without a refcount -- tests "rule name != subvol"
            os.makedirs(subs_dir / "no:refs")
            tmp_subvols.create("no:refs/subvol_name")
            gcd_subs.add(Path("no:refs"))

            # Wrapper without a refcount and without a subvolume
            os.makedirs((subs_dir / "no_refs:nor_subvol"))
            gcd_subs.add(Path("no_refs:nor_subvol"))

            # Subvolume, whose refcount is 1
            (refs_dir / "1:link.json").touch()
            os.makedirs(subs_dir / "1:link")
            tmp_subvols.create("1:link/1")

            gcd_refs.add(Path("1:link.json"))
            gcd_subs.add(Path("1:link"))

            # Some refcount files with a link count of 2
            (refs_dir / "2link:1.json").touch()
            os.link(refs_dir / "2link:1.json", refs_dir / "2link:2.json")
            kept_refs.add(Path("2link:1.json"))
            kept_refs.add(Path("2link:2.json"))

            # Subvolumes for both of the 2-link refcount files
            os.makedirs(subs_dir / "2link:1")
            tmp_subvols.create("2link:1/2link")
            os.makedirs(subs_dir / "2link:2")
            tmp_subvols.create("2link:2/2link")
            kept_subs.add(Path("2link:1"))
            kept_subs.add(Path("2link:2"))

            # Some refcount files with a link count of 3
            three_link = refs_dir / "3link:1.json"
            three_link.touch()
            os.link(three_link, refs_dir / "3link:2.json")
            os.link(three_link, refs_dir / "3link:3.json")
            kept_refs.add(Path("3link:1.json"))
            kept_refs.add(Path("3link:2.json"))
            kept_refs.add(Path("3link:3.json"))

            # Make a subvolume for 1 of them, it won't get GC'd
            os.makedirs(subs_dir / "3link:2")
            tmp_subvols.create("3link:2/3link")
            kept_subs.add(Path("3link:2"))

            self.assertEqual(kept_refs | gcd_refs, set(refs_dir.listdir()))
            self.assertEqual(kept_subs | gcd_subs, set(subs_dir.listdir()))

            yield sgc.argparse.Namespace(
                gcd_subs=gcd_subs,
                kept_subs=kept_subs,
                gcd_refs=gcd_refs,
                kept_refs=kept_refs,
                refs_dir=refs_dir,
                subs_dir=subs_dir,
            )

    # pyre-fixme[2]: Parameter must be annotated.
    def _gc_only(self, n) -> None:
        sgc.subvolume_garbage_collector(
            [f"--refcounts-dir={n.refs_dir}", f"--subvolumes-dir={n.subs_dir}"]
        )

    def test_garbage_collect_subvolumes(self) -> None:
        for fn in [
            lambda n: sgc.garbage_collect_subvolumes(n.refs_dir, n.subs_dir),
            self._gc_only,
        ]:
            with self._gc_test_case() as n:
                fn(n)
                self.assertEqual(n.kept_refs, set(n.refs_dir.listdir()))
                self.assertEqual(n.kept_subs, set(n.subs_dir.listdir()))

    def test_no_gc_due_to_lock(self) -> None:
        with self._gc_test_case() as n:
            fd = os.open(n.subs_dir, os.O_RDONLY)
            try:
                fcntl.flock(fd, fcntl.LOCK_SH | fcntl.LOCK_NB)
                self._gc_only(n)

                # Sneak in a test that new subvolume creation fails when
                # its refcount already exists.
                with temp_dir() as json_dir, self.assertRaisesRegex(
                    RuntimeError, "Refcount already exists:"
                ):
                    sgc.subvolume_garbage_collector(
                        [
                            f"--refcounts-dir={n.refs_dir}",
                            f"--subvolumes-dir={n.subs_dir}",
                            # This refcount was created by `_gc_test_case`.
                            "--new-subvolume-wrapper-dir=3link:1",
                            f'--new-subvolume-json={json_dir / "OUT"}',
                        ]
                    )

            finally:
                os.close(fd)

            self.assertEqual(
                n.kept_refs | n.gcd_refs, set(n.refs_dir.listdir())
            )
            self.assertEqual(
                n.kept_subs | n.gcd_subs, set(n.subs_dir.listdir())
            )

    def test_garbage_collect_and_make_new_subvolume(self) -> None:
        with self._gc_test_case() as n, temp_dir() as json_dir:
            sgc.subvolume_garbage_collector(
                [
                    f"--refcounts-dir={n.refs_dir}",
                    f"--subvolumes-dir={n.subs_dir}",
                    "--new-subvolume-wrapper-dir=new:subvol",
                    f'--new-subvolume-json={json_dir / "OUT"}',
                ]
            )
            self.assertEqual([b"OUT"], json_dir.listdir())
            self.assertEqual(
                n.kept_refs | {Path("new:subvol.json")},
                set(n.refs_dir.listdir()),
            )
            self.assertEqual(
                n.kept_subs | {Path("new:subvol")},
                set(n.subs_dir.listdir()),
            )


if __name__ == "__main__":
    unittest.main()
