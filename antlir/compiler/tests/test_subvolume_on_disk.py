#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
import os
import unittest
import unittest.mock
from uuid import UUID

from antlir.btrfsutil import BtrfsUtilError, subvolume_info
from antlir.fs_utils import Path
from antlir.subvol_utils import with_temp_subvols

from .. import subvolume_on_disk

_MY_HOST = "my_host"


class SubvolumeOnDiskTestCase(unittest.TestCase):
    def setUp(self) -> None:
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

        self.patch_gethostname = unittest.mock.patch("socket.gethostname")
        self.mock_gethostname = self.patch_gethostname.start()
        self.mock_gethostname.side_effect = lambda: _MY_HOST
        self.addCleanup(self.patch_gethostname.stop)

    # pyre-fixme[2]: Parameter must be annotated.
    def _check(self, actual_subvol, expected_path, expected_subvol) -> None:
        self.assertEqual(expected_path, actual_subvol.subvolume_path())
        self.assertEqual(expected_subvol, actual_subvol)

        # Automatically tests "normal case" serialization & deserialization
        fake_file = io.StringIO()

        actual_subvol.to_json_file(fake_file)
        fake_file.seek(0)

        self.assertEqual(
            actual_subvol,
            subvolume_on_disk.SubvolumeOnDisk.from_json_file(
                fake_file, actual_subvol.subvolumes_base_dir
            ),
        )

    def test_from_json_file_errors(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "Parsing subvolume JSON"):
            subvolume_on_disk.SubvolumeOnDisk.from_json_file(
                io.StringIO("invalid json"), Path("/subvols")
            )
        with self.assertRaisesRegex(RuntimeError, "Parsed subvolume JSON"):
            subvolume_on_disk.SubvolumeOnDisk.from_json_file(
                io.StringIO("5"), Path("/subvols")
            )

    @with_temp_subvols
    # pyre-fixme[2]: Parameter must be annotated.
    def test_from_serializable_dict_and_validation(self, temp_subvols) -> None:
        # Note: Unlike test_from_subvolume_path, this test uses a
        # trailing / (to increase coverage).
        subvols = Path(temp_subvols.temp_dir + b"/")
        rel_path = Path("test_subvol:v/test_subvol")
        good_path = subvols / rel_path
        temp_subvols.create(rel_path)
        info = subvolume_info(good_path)
        good_uuid = str(UUID(bytes=info.uuid))
        good = {
            subvolume_on_disk._BTRFS_UUID: good_uuid,
            subvolume_on_disk._HOSTNAME: _MY_HOST,
            subvolume_on_disk._SUBVOLUME_REL_PATH: rel_path,
        }

        bad_path = good.copy()
        # pyre-ignore[16]: Item `str` of `typing.Union[Path, str]` has no attribute `__itruediv__`.
        bad_path[subvolume_on_disk._SUBVOLUME_REL_PATH] /= "x"
        bad_path_subvol = temp_subvols.create(
            bad_path[subvolume_on_disk._SUBVOLUME_REL_PATH]
        )
        with self.assertRaisesRegex(RuntimeError, "must have the form"):
            subvolume_on_disk.SubvolumeOnDisk.from_serializable_dict(
                bad_path, subvols
            )
        bad_path_subvol.delete()

        wrong_inner = good.copy()
        wrong_inner_relpath = (
            wrong_inner[subvolume_on_disk._SUBVOLUME_REL_PATH]
            + b"x"  # pyre-ignore[6]
        )
        wrong_inner[subvolume_on_disk._SUBVOLUME_REL_PATH] = wrong_inner_relpath
        wrong_inner_subvol = temp_subvols.create(wrong_inner_relpath)
        with self.assertRaisesRegex(
            RuntimeError,
            r"\[b'test_subvol'\, b'test_subvolx'\] instead of \[b'test_subvolx'\]",
        ):
            subvolume_on_disk.SubvolumeOnDisk.from_serializable_dict(
                wrong_inner, subvols
            )
        wrong_inner_subvol.delete()

        wrong_uuid = good.copy()
        wrong_uuid[
            subvolume_on_disk._BTRFS_UUID
        ] = "fbe20093-17f1-4ffa-86fb-54e112094f6f"
        with self.assertRaisesRegex(
            RuntimeError, "UUID in subvolume JSON .* does not match"
        ):
            subvolume_on_disk.SubvolumeOnDisk.from_serializable_dict(
                wrong_uuid, subvols
            )

        # Parsing the `good` dict does not throw, and gets the right result
        good_sv = subvolume_on_disk.SubvolumeOnDisk.from_serializable_dict(
            good, subvols
        )
        self._check(
            good_sv,
            good_path,
            subvolume_on_disk.SubvolumeOnDisk(
                **{
                    subvolume_on_disk._BTRFS_UUID: good_uuid,
                    subvolume_on_disk._BTRFS_PARENT_UUID: None,
                    subvolume_on_disk._HOSTNAME: _MY_HOST,
                    subvolume_on_disk._SUBVOLUME_REL_PATH: rel_path,
                    subvolume_on_disk._SUBVOLUMES_BASE_DIR: subvols,
                }
            ),
        )

    @with_temp_subvols
    # pyre-fixme[2]: Parameter must be annotated.
    def test_from_subvolume_path(self, temp_subvols) -> None:
        # Note: Unlike test_from_serializable_dict_and_validation, this
        # test does NOT use a trailing / (to increase coverage).
        subvols = Path(temp_subvols.temp_dir.rstrip(b"/"))
        rel_path = Path("test_rule:vvv/test:subvol")
        temp_subvols.create(rel_path)
        subvol_path = subvols / rel_path
        build_appliance_path = Path("build_appliance")
        uuid = str(UUID(bytes=subvolume_info(subvol_path).uuid))

        subvol = subvolume_on_disk.SubvolumeOnDisk.from_subvolume_path(
            subvol_path=subvol_path,
            subvolumes_dir=subvols,
            build_appliance_path=build_appliance_path,
        )
        with unittest.mock.patch("os.listdir") as listdir:
            listdir.return_value = ["test:subvol"]
            self._check(
                subvol,
                subvol_path,
                subvolume_on_disk.SubvolumeOnDisk(
                    **{
                        subvolume_on_disk._BTRFS_UUID: uuid,
                        subvolume_on_disk._BTRFS_PARENT_UUID: None,
                        subvolume_on_disk._HOSTNAME: _MY_HOST,
                        subvolume_on_disk._SUBVOLUME_REL_PATH: rel_path,
                        subvolume_on_disk._SUBVOLUMES_BASE_DIR: subvols,
                        subvolume_on_disk._BUILD_APPLIANCE_PATH: (
                            build_appliance_path
                        ),
                    }
                ),
            )
            self.assertEqual(
                listdir.call_args_list,
                [((os.path.dirname(subvol_path),),)] * 2,
            )

        with self.assertRaisesRegex(
            RuntimeError, "must be located inside the subvolumes directory"
        ):
            subvolume_on_disk.SubvolumeOnDisk.from_subvolume_path(
                subvol_path=subvol_path,
                subvolumes_dir=subvols / "bad",
            )


if __name__ == "__main__":
    unittest.main()
