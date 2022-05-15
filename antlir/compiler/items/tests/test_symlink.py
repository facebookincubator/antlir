#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import tempfile

from antlir.compiler.requires_provides import (
    ProvidesSymlink,
    RequireDirectory,
    RequireFile,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol, TempSubvolumes

from ..install_file import InstallFileItem
from ..symlink import SymlinkToDirItem, SymlinkToFileItem
from .common import (
    BaseItemTestCase,
    DUMMY_LAYER_OPTS,
    get_dummy_layer_opts_ba,
    render_subvol,
    with_mocked_temp_volume_dir,
)


DUMMY_LAYER_OPTS_BA = get_dummy_layer_opts_ba(
    Subvol("test-build-appliance", already_exists=True)
)


class SymlinkItemsTestCase(BaseItemTestCase):
    def test_symlink(self) -> None:
        self._check_item(
            SymlinkToDirItem(from_target="t", source="x", dest="y"),
            {ProvidesSymlink(path=Path("y"), target=Path("x"))},
            {
                RequireDirectory(path=Path("/")),
                RequireDirectory(path=Path("/x")),
            },
        )

        self._check_item(
            SymlinkToFileItem(
                from_target="t", source="source_file", dest="dest_symlink"
            ),
            {
                ProvidesSymlink(
                    path=Path("dest_symlink"), target=Path("source_file")
                )
            },
            {
                RequireDirectory(path=Path("/")),
                RequireFile(path=Path("/source_file")),
            },
        )

    @with_mocked_temp_volume_dir
    def test_symlink_idempotent(self) -> None:
        with TempSubvolumes() as ts:
            sv = ts.create("test")
            sv.run_as_root(["touch", sv.path("a")])
            sv.run_as_root(["mkdir", sv.path("x")])
            SymlinkToFileItem(from_target="t", source="a", dest="b").build(
                sv, DUMMY_LAYER_OPTS
            )
            SymlinkToDirItem(from_target="t", source="x", dest="y").build(
                sv, DUMMY_LAYER_OPTS
            )
            sv.set_readonly(True)
            SymlinkToFileItem(from_target="t", source="a", dest="b").build(
                sv, DUMMY_LAYER_OPTS
            )
            SymlinkToDirItem(from_target="t", source="x", dest="y").build(
                sv, DUMMY_LAYER_OPTS
            )

    @with_mocked_temp_volume_dir
    def test_symlink_already_exists(self) -> None:
        with TempSubvolumes() as ts:
            sv = ts.create("test")
            sv.run_as_root(["touch", sv.path("a")])
            sv.run_as_root(["touch", sv.path("b")])
            sv.set_readonly(True)
            with self.assertRaises(
                RuntimeError, msg="dest='b' source='c': dest already exists"
            ):
                SymlinkToFileItem(from_target="t", source="a", dest="b").build(
                    sv, DUMMY_LAYER_OPTS
                )

    @with_mocked_temp_volume_dir
    def test_symlink_already_matches(self) -> None:
        with TempSubvolumes() as ts:
            sv = ts.create("test")
            sv.run_as_root(["touch", sv.path("a")])
            sv.run_as_root(["ln", "-ns", "a", sv.path("b")])
            sv.set_readonly(True)
            SymlinkToFileItem(from_target="t", source="a", dest="b").build(
                sv, DUMMY_LAYER_OPTS
            )

    @with_mocked_temp_volume_dir
    def test_symlink_already_exists_different_source(self) -> None:
        with TempSubvolumes() as ts:
            sv = ts.create("test")
            sv.run_as_root(["touch", sv.path("a")])
            SymlinkToFileItem(from_target="t", source="a", dest="b").build(
                sv, DUMMY_LAYER_OPTS
            )
            sv.set_readonly(True)
            with self.assertRaises(
                RuntimeError, msg="dest='b' source='c': b -> a exists to b'a'"
            ):
                SymlinkToFileItem(from_target="t", source="c", dest="b").build(
                    sv, DUMMY_LAYER_OPTS
                )

    @with_mocked_temp_volume_dir
    def _test_symlink_command(self, layer_opts) -> None:
        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.create("tar-sv")
            subvol.run_as_root(["mkdir", subvol.path("dir")])

            # We need a source file to validate a SymlinkToFileItem
            with tempfile.NamedTemporaryFile() as tf:
                InstallFileItem(
                    from_target="t", source=tf.name, dest="/file"
                ).build(subvol, layer_opts)

            SymlinkToDirItem(
                from_target="t", source="/dir", dest="/dir_symlink"
            ).build(subvol, layer_opts)
            SymlinkToFileItem(
                from_target="t", source="file", dest="/file_symlink"
            ).build(subvol, layer_opts)

            # Make a couple of absolute symlinks to test our behavior on
            # linking to paths that contain those.
            subvol.run_as_root(
                [
                    "bash",
                    "-c",
                    f"""\
                ln -s /file {subvol.path('abs_link_to_file').shell_quote()}
                mkdir {subvol.path('my_dir').shell_quote()}
                touch {subvol.path('my_dir/inner').shell_quote()}
                ln -s /my_dir {subvol.path('my_dir_link').shell_quote()}
            """,
                ]
            )
            # A simple case: we link to an absolute link.
            SymlinkToFileItem(
                from_target="t",
                source="/abs_link_to_file",
                dest="/link_to_abs_link",
            ).build(subvol, layer_opts)
            # This link traverses a directory that is an absolute link.  The
            # resulting relative symlink is not traversible from outside the
            # container.
            SymlinkToFileItem(
                from_target="t",
                source="my_dir_link/inner",
                dest="/dir/inner_link",
            ).build(subvol, layer_opts)

            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "dir": [
                            "(Dir)",
                            {"inner_link": ["(Symlink ../my_dir_link/inner)"]},
                        ],
                        "dir_symlink": ["(Symlink dir)"],
                        "file": ["(File m444)"],
                        "file_symlink": ["(Symlink file)"],
                        "abs_link_to_file": ["(Symlink /file)"],
                        "my_dir": ["(Dir)", {"inner": ["(File)"]}],
                        "my_dir_link": ["(Symlink /my_dir)"],
                        "link_to_abs_link": ["(Symlink abs_link_to_file)"],
                    },
                ],
                render_subvol(subvol),
            )

    def test_symlink_command_non_ba(self) -> None:
        self._test_symlink_command(DUMMY_LAYER_OPTS)

    def test_symlink_command_ba(self) -> None:
        self._test_symlink_command(DUMMY_LAYER_OPTS_BA)
