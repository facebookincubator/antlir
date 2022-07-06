#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import stat
import subprocess
import sys
import tempfile

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesFile,
    RequireDirectory,
    RequireGroup,
    RequireUser,
)
from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import Path, temp_dir
from antlir.subvol_utils import TempSubvolumes

from ..common import image_source_item
from ..install_file import _InstallablePath, InstallFileItem
from .common import BaseItemTestCase, DUMMY_LAYER_OPTS, render_subvol


def _install_file_item(**kwargs):
    # The dummy object works here because `subvolumes_dir` of `None` runs
    # `artifacts_dir` internally, while our "prod" path uses the
    # already-computed value.
    return image_source_item(
        InstallFileItem, exit_stack=None, layer_opts=DUMMY_LAYER_OPTS
    )(**kwargs)


class InstallFileItemTestCase(BaseItemTestCase):
    def test_phase_order(self):
        self.assertIs(
            None,
            InstallFileItem(
                from_target="t", source="/etc/passwd", dest="b"
            ).phase_order(),
        )

    def test_install_file(self):
        with tempfile.NamedTemporaryFile() as tf:
            os.chmod(tf.name, stat.S_IXUSR)
            exe_item = _install_file_item(
                from_target="t", source={"source": tf.name}, dest="d/c"
            )
        ep = _InstallablePath(
            Path(tf.name), ProvidesFile(path=Path("d/c")), 0o555
        )
        self.assertEqual((ep,), exe_item._paths)
        self.assertEqual(tf.name.encode(), exe_item.source)
        self._check_item(
            exe_item,
            {ep.provides},
            {
                RequireDirectory(path=Path("d")),
                RequireUser("root"),
                RequireGroup("root"),
            },
        )

        # Checks `image.source(path=...)`
        with temp_dir() as td:
            os.mkdir(td / "b")
            open(td / "b/q", "w").close()
            data_item = _install_file_item(
                from_target="t", source={"source": td, "path": "/b/q"}, dest="d"
            )
        dp = _InstallablePath(td / "b/q", ProvidesFile(path=Path("d")), 0o444)
        self.assertEqual((dp,), data_item._paths)
        self.assertEqual(td / "b/q", data_item.source)
        self._check_item(
            data_item,
            {dp.provides},
            {
                RequireDirectory(path=Path("/")),
                RequireUser("root"),
                RequireGroup("root"),
            },
        )

        # NB: We don't need to get coverage for this check on ALL the items
        # because the presence of the ProvidesDoNotAccess items it the real
        # safeguard -- e.g. that's what prevents TarballItem from writing
        # to /.meta/ or other protected paths.
        with self.assertRaisesRegex(AssertionError, "cannot start with .meta/"):
            _install_file_item(
                from_target="t", source={"source": "a/b/c"}, dest="/.meta/foo"
            )

    def test_install_file_from_layer(self):
        layer = find_built_subvol(
            Path(__file__).dirname() / "test-with-one-local-rpm"
        )
        path_in_layer = b"rpm_test/cheese2.txt"
        item = _install_file_item(
            from_target="t",
            source={"layer": layer, "path": "/" + path_in_layer.decode()},
            dest="cheese2",
        )
        source_path = layer.path(path_in_layer)
        p = _InstallablePath(
            source_path, ProvidesFile(path=Path("cheese2")), 0o444
        )
        self.assertEqual((p,), item._paths)
        self.assertEqual(source_path, item.source)
        self._check_item(
            item,
            {p.provides},
            {
                RequireDirectory(path=Path("/")),
                RequireUser("root"),
                RequireGroup("root"),
            },
        )

    def test_install_file_command(self):
        with TempSubvolumes(
            Path(sys.argv[0])
        ) as temp_subvolumes, tempfile.NamedTemporaryFile() as empty_tf:
            subvol = temp_subvolumes.create("tar-sv")
            subvol.run_as_root(["mkdir", subvol.path("d")])

            _install_file_item(
                from_target="t",
                source={"source": empty_tf.name},
                dest="/d/empty",
            ).build(subvol, DUMMY_LAYER_OPTS)
            self.assertEqual(
                ["(Dir)", {"d": ["(Dir)", {"empty": ["(File m444)"]}]}],
                render_subvol(subvol),
            )

            # Fail to write to a nonexistent dir
            with self.assertRaises(subprocess.CalledProcessError):
                _install_file_item(
                    from_target="t",
                    source={"source": empty_tf.name},
                    dest="/no_dir/empty",
                ).build(subvol, DUMMY_LAYER_OPTS)

            # Running a second copy to the same destination. This just
            # overwrites the previous file, because we have a build-time
            # check for this, and a run-time check would add overhead.
            _install_file_item(
                from_target="t",
                source={"source": empty_tf.name},
                dest="/d/empty",
                # A non-default mode & owner shows that the file was
                # overwritten, and also exercises HasStatOptions.
                mode=0o600,
                user="12",
                group="34",
            ).build(subvol, DUMMY_LAYER_OPTS)
            self.assertEqual(
                ["(Dir)", {"d": ["(Dir)", {"empty": ["(File m600 o12:34)"]}]}],
                render_subvol(subvol),
            )

    def test_install_file_unsupported_types(self):
        with self.assertRaisesRegex(
            RuntimeError, " must be a regular file or directory, "
        ):
            _install_file_item(
                from_target="t", source={"source": "/dev/null"}, dest="d/c"
            )
        with self.assertRaisesRegex(RuntimeError, " neither a file nor a dir"):
            _install_file_item(
                from_target="t", source={"source": "/dev"}, dest="d/c"
            )

    def test_install_file_command_recursive(self):
        with TempSubvolumes(Path(sys.argv[0])) as temp_subvolumes:
            subvol = temp_subvolumes.create("tar-sv")
            subvol.run_as_root(["mkdir", subvol.path("d")])

            with temp_dir() as td:
                with open(td / "data.txt", "w") as df:
                    print("Hello", file=df)
                os.mkdir(td / "subdir")
                with open(td / "subdir/exe.sh", "w") as ef:
                    print('#!/bin/sh\necho "Hello"', file=ef)
                os.chmod(td / "subdir/exe.sh", 0o100)

                dir_item = _install_file_item(
                    from_target="t", source={"source": td}, dest="/d/a"
                )

                ps = [
                    _InstallablePath(
                        td, ProvidesDirectory(path=Path("d/a")), 0o755
                    ),
                    _InstallablePath(
                        td / "data.txt",
                        ProvidesFile(path=Path("d/a/data.txt")),
                        0o444,
                    ),
                    _InstallablePath(
                        td / "subdir",
                        ProvidesDirectory(path=Path("d/a/subdir")),
                        0o755,
                    ),
                    _InstallablePath(
                        td / "subdir/exe.sh",
                        ProvidesFile(path=Path("d/a/subdir/exe.sh")),
                        0o555,
                    ),
                ]
                self.assertEqual(sorted(ps), sorted(dir_item._paths))
                self.assertEqual(td, dir_item.source)
                self._check_item(
                    dir_item,
                    {p.provides for p in ps},
                    {
                        RequireDirectory(path=Path("d")),
                        RequireUser("root"),
                        RequireGroup("root"),
                    },
                )

                # This implicitly checks that `a` precedes its contents.
                dir_item.build(subvol, DUMMY_LAYER_OPTS)

            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "d": [
                            "(Dir)",
                            {
                                "a": [
                                    "(Dir)",
                                    {
                                        "data.txt": ["(File m444 d6)"],
                                        "subdir": [
                                            "(Dir)",
                                            {"exe.sh": ["(File m555 d23)"]},
                                        ],
                                    },
                                ]
                            },
                        ]
                    },
                ],
                render_subvol(subvol),
            )

    def test_install_file_large_batched_chmod(self):
        # Create a large number of files with long names to intentionally
        # overflow the normal size limit of the chmod call
        with temp_dir() as td:
            arg_max = os.sysconf(os.sysconf_names["SC_ARG_MAX"])
            paths = [
                f"{td}/{i:0120d}"
                for i in range(10 + arg_max // (len(td) + 1 + 120))
            ]
            # simple check that the large exec behavior would otherwise be
            # triggered by this test case
            self.assertGreater(len("\0".join(paths)), arg_max)
            for path in paths:
                open(path, "w").close()

            dir_item = _install_file_item(
                from_target="t",
                source={"source": td},
                dest="/d",
            )

            with TempSubvolumes(Path(sys.argv[0])) as temp_subvolumes:
                subvol = temp_subvolumes.create("large-chmod")
                dir_item.build(subvol, DUMMY_LAYER_OPTS)

                self.assertEqual(
                    [
                        "(Dir)",
                        {
                            "d": [
                                "(Dir)",
                                {
                                    os.path.basename(p): ["(File m444)"]
                                    for p in paths
                                },
                            ]
                        },
                    ],
                    render_subvol(subvol),
                )
