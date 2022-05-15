#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import copy
import os
import subprocess
import sys
import tarfile
import tempfile
from contextlib import ExitStack

from antlir.compiler.requires_provides import RequireDirectory
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes

from ..common import _hash_path, image_source_item
from ..tarball import TarballItem
from .common import (
    BaseItemTestCase,
    DUMMY_LAYER_OPTS,
    get_dummy_layer_opts_ba,
    render_subvol,
    temp_filesystem,
    temp_filesystem_provides,
)


DUMMY_LAYER_OPTS_BA = get_dummy_layer_opts_ba()


def _tarball_item(
    tarball: str, into_dir: str, force_root_ownership: bool = False
) -> TarballItem:
    "Constructs a common-case TarballItem"
    return image_source_item(
        TarballItem, exit_stack=None, layer_opts=DUMMY_LAYER_OPTS
    )(
        from_target="t",
        into_dir=into_dir,
        source={
            "source": tarball,
            "content_hash": "sha256:" + _hash_path(tarball, "sha256"),
        },
        force_root_ownership=force_root_ownership,
    )


def _tarinfo_strip_dir_prefix(dir_prefix):
    "Returns a `filter=` for `TarFile.add`"
    dir_prefix = dir_prefix.lstrip("/")

    def strip_dir_prefix(tarinfo):
        if tarinfo.path.startswith(dir_prefix + "/"):
            tarinfo.path = tarinfo.path[len(dir_prefix) + 1 :]
        elif dir_prefix == tarinfo.path:
            tarinfo.path = "."
        else:
            raise AssertionError(f"{tarinfo.path} must start with {dir_prefix}")
        return tarinfo

    return strip_dir_prefix


class TarballItemTestCase(BaseItemTestCase):
    def test_tarball(self):
        with temp_filesystem() as fs_path, tempfile.TemporaryDirectory() as td:
            tar_path = os.path.join(td, "test.tar")
            zst_path = os.path.join(td, "test.tar.zst")
            bzip2_path = os.path.join(td, "test.tar.bz2")

            with tarfile.TarFile(tar_path, "w") as tar_obj:
                tar_obj.add(fs_path, filter=_tarinfo_strip_dir_prefix(fs_path))
            subprocess.check_call(["zstd", tar_path, "-o", zst_path])
            subprocess.check_call(["bzip2", "-zk", tar_path])

            for path in (tar_path, zst_path, bzip2_path):
                self._check_item(
                    _tarball_item(path, "y"),
                    temp_filesystem_provides("y"),
                    {RequireDirectory(path=Path("y"))},
                )

            # Test a hash validation failure, follows the item above
            with self.assertRaisesRegex(AssertionError, "failed hash vali"):
                image_source_item(
                    TarballItem, exit_stack=None, layer_opts=DUMMY_LAYER_OPTS
                )(
                    from_target="t",
                    into_dir="y",
                    source={
                        "source": tar_path,
                        "content_hash": "sha256:deadbeef",
                    },
                    force_root_ownership=False,
                )

    # NB: We don't need to test `build` because TarballItem has no logic
    # specific to generated vs pre-built tarballs.  It would really be
    # enough just to construct the item, but it was easy to test `provides`.
    def test_tarball_generator(self):
        with temp_filesystem() as fs_path, tempfile.NamedTemporaryFile() as t, ExitStack() as exit_stack:  # noqa: E501
            with tarfile.TarFile(t.name, "w") as tar_obj:
                tar_obj.add(fs_path, filter=_tarinfo_strip_dir_prefix(fs_path))
            self._check_item(
                image_source_item(
                    TarballItem,
                    exit_stack=exit_stack,
                    layer_opts=DUMMY_LAYER_OPTS,
                )(
                    from_target="t",
                    into_dir="y",
                    source={
                        "generator": "/bin/bash",
                        "generator_args": [
                            "-c",
                            'cp "$1" "$2"; basename "$1"',
                            "test_tarball_generator",  # $0
                            t.name,  # $1, making $2 the output directory
                        ],
                        "content_hash": "sha256:"
                        + _hash_path(t.name, "sha256"),
                    },
                    force_root_ownership=False,
                ),
                temp_filesystem_provides("y"),
                {RequireDirectory(path=Path("y"))},
            )

    def test_tarball_command(self):
        with TempSubvolumes(Path(sys.argv[0])) as temp_subvolumes:
            subvol = temp_subvolumes.create("tar-sv")
            subvol.run_as_root(["mkdir", subvol.path("d")])

            # Fail on pre-existing files
            subvol.run_as_root(["touch", subvol.path("d/exists")])
            with tempfile.NamedTemporaryFile() as t:
                with tarfile.TarFile(t.name, "w") as tar_obj:
                    tar_obj.addfile(tarfile.TarInfo("exists"))
                with self.assertRaises(subprocess.CalledProcessError):
                    _tarball_item(t.name, "/d").build(
                        subvol, DUMMY_LAYER_OPTS_BA
                    )

            # Adding new files & directories works. Overwriting a
            # pre-existing directory leaves the owner+mode of the original
            # directory intact.
            subvol.run_as_root(["mkdir", subvol.path("d/old_dir")])
            subvol.run_as_root(["chown", "123:456", subvol.path("d/old_dir")])
            subvol.run_as_root(["chmod", "0301", subvol.path("d/old_dir")])
            subvol_root = temp_subvolumes.snapshot(subvol, "tar-sv-root")
            subvol_zst = temp_subvolumes.snapshot(subvol, "tar-sv-zst")
            with tempfile.TemporaryDirectory() as td:
                tar_path = os.path.join(td, "test.tar")
                zst_path = os.path.join(td, "test.tar.zst")
                with tarfile.TarFile(tar_path, "w") as tar_obj:
                    tar_obj.addfile(tarfile.TarInfo("new_file"))

                    new_dir = tarfile.TarInfo("new_dir")
                    new_dir.type = tarfile.DIRTYPE
                    new_dir.uid = 12
                    new_dir.gid = 34
                    tar_obj.addfile(new_dir)

                    old_dir = tarfile.TarInfo("old_dir")
                    old_dir.type = tarfile.DIRTYPE
                    # These will not be applied because old_dir exists
                    old_dir.uid = 0
                    old_dir.gid = 0
                    old_dir.mode = 0o755
                    tar_obj.addfile(old_dir)

                subprocess.check_call(["zstd", tar_path, "-o", zst_path])

                # Fail when the destination does not exist
                with self.assertRaises(subprocess.CalledProcessError):
                    _tarball_item(tar_path, "/no_dir").build(
                        subvol, DUMMY_LAYER_OPTS_BA
                    )

                # Before unpacking the tarball
                orig_content = [
                    "(Dir)",
                    {
                        "d": [
                            "(Dir)",
                            {
                                "exists": ["(File)"],
                                "old_dir": ["(Dir m301 o123:456)", {}],
                            },
                        ]
                    },
                ]
                # After unpacking `tar_path` in `/d`.
                new_content = copy.deepcopy(orig_content)
                new_content[1]["d"][1].update(
                    {
                        "new_dir": ["(Dir m644 o12:34)", {}],
                        "new_file": ["(File)"],
                    }
                )
                # After unpacking `tar_path` in `/d` with `force_root_ownership`
                new_content_root = copy.deepcopy(new_content)
                # The ownership of 12:34 is gone.
                new_content_root[1]["d"][1]["new_dir"] = ["(Dir m644)", {}]
                self.assertNotEqual(new_content, new_content_root)

                # Check the subvolume content before and after unpacking
                for item, (sv, before, after) in (
                    (
                        _tarball_item(tar_path, "/d/"),
                        (subvol, orig_content, new_content),
                    ),
                    (
                        _tarball_item(tar_path, "d", force_root_ownership=True),
                        (subvol_root, orig_content, new_content_root),
                    ),
                    (
                        _tarball_item(zst_path, "d/"),
                        (subvol_zst, orig_content, new_content),
                    ),
                ):
                    self.assertEqual(before, render_subvol(sv))
                    item.build(sv, DUMMY_LAYER_OPTS_BA)
                    self.assertEqual(after, render_subvol(sv))
