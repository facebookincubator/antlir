#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import tempfile
from typing import Any, Dict, List, Optional, Union

from antlir.compiler.items.make_subvol import FilesystemRootItem, ParentLayerItem
from antlir.compiler.items.mount import (
    BuildSource,
    Mount,
    MountItem,
    mounts_from_meta,
    RuntimeSource,
)
from antlir.compiler.items.phases_provide import PhasesProvideItem
from antlir.compiler.items.tests.common import (
    BaseItemTestCase,
    DUMMY_LAYER_OPTS,
    render_subvol,
    with_mocked_temp_volume_dir,
)

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesGroup,
    ProvidesUser,
    RequireDirectory,
)
from antlir.compiler.subvolume_on_disk import SubvolumeOnDisk
from antlir.config import antlir_dep
from antlir.fs_utils import Path, temp_dir
from antlir.subvol_utils import Subvol, TempSubvolumes
from antlir.tests.subvol_helpers import get_meta_dir_contents


def _mount_item_new(from_target: str, mount_config: Dict[str, Any]) -> MountItem:
    return MountItem(
        layer_opts=DUMMY_LAYER_OPTS._replace(
            allowed_host_mount_targets=["foo//dummy/host_mounts:t"]
        ),
        from_target=from_target,
        mountpoint="/lala",
        target=None,
        mount_config=mount_config,
    )


class MountItemTestCase(BaseItemTestCase):
    @with_mocked_temp_volume_dir
    def test_mount_item_file_from_host(self) -> None:
        mount_config = {
            "is_directory": False,
            "build_source": {"type": "host", "source": "/dev/null"},
        }

        with self.assertRaisesRegex(AssertionError, "must be sourced directly"):
            _mount_item_new("foo//bar:baz", mount_config)

        bad_mount_config = mount_config.copy()
        bad_mount_config["runtime_source"] = bad_mount_config["build_source"]
        with self.assertRaisesRegex(AssertionError, "Only `build_source` may "):
            _mount_item_new("foo//dummy/host_mounts:t", bad_mount_config)

        mount_item = _mount_item_new("foo//dummy/host_mounts:t", mount_config)

        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.create("mounter")
            mount_item.build(
                subvol,
                DUMMY_LAYER_OPTS._replace(target_to_path={}, subvolumes_dir="unused"),
            )

            mount = [
                "(Dir)",
                {
                    "lala": [
                        "(Dir)",
                        {
                            "MOUNT": [
                                "(Dir)",
                                {
                                    "is_directory": ["(File d2)"],
                                    "build_source": [
                                        "(Dir)",
                                        {
                                            "type": ["(File d5)"],
                                            "source": [f'(File d{len("/dev/null")+1})'],
                                        },
                                    ],
                                },
                            ]
                        },
                    ]
                },
            ]
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "lala": ["(File)"],  # An empty mountpoint for /dev/null
                        ".meta": [
                            "(Dir)",
                            {
                                "private": [
                                    "(Dir)",
                                    {
                                        # No `opts/artifacts_may_require_repo`
                                        # here because we directly created the
                                        # subvol instead of using an Item.
                                        "mount": mount
                                    },
                                ]
                            },
                        ],
                    },
                ],
                render_subvol(subvol),
            )
            for filename, contents in (
                ("is_directory", "0\n"),
                ("build_source/type", "host\n"),
                ("build_source/source", "/dev/null\n"),
            ):
                self.assertEqual(
                    contents,
                    subvol.read_path_text(
                        Path(".meta/private/mount/lala/MOUNT") / filename
                    ),
                )

    def _make_mount_item(
        self,
        *,
        mountpoint: Optional[str],
        target: str,
        mount_config: Dict[str, Any],
        from_target: str = "t",
    ) -> MountItem:
        "Ensures that `target` and `mount_config` make the same item."
        item_from_file = MountItem(
            layer_opts=DUMMY_LAYER_OPTS,
            from_target=from_target,
            mountpoint=mountpoint,
            target=target,
            mount_config=None,
        )
        self.assertEqual(
            item_from_file,
            MountItem(
                layer_opts=DUMMY_LAYER_OPTS,
                from_target=from_target,
                mountpoint=mountpoint,
                target=None,
                mount_config=mount_config,
            ),
        )
        return item_from_file

    def test_mount_item_default_mountpoint(self) -> None:
        with tempfile.TemporaryDirectory() as mnt_target:
            mount_config: Dict[str, Any] = {
                "is_directory": True,
                "build_source": {"type": "layer", "source": "//fake:path"},
            }
            with open(os.path.join(mnt_target, "mountconfig.json"), "w") as f:
                json.dump(mount_config, f)
            # Since our initial mountconfig lacks `default_mountpoint`, the
            # item requires its `mountpoint` to be set.
            with self.assertRaisesRegex(AssertionError, "lacks mountpoint"):
                MountItem(
                    layer_opts=DUMMY_LAYER_OPTS,
                    from_target="t",
                    mountpoint=None,
                    target=mnt_target,
                    mount_config=None,
                )

            # Now, check that the default gets used.
            mount_config["default_mountpoint"] = "potato"
            with open(os.path.join(mnt_target, "mountconfig.json"), "w") as f:
                json.dump(mount_config, f)
            self.assertEqual(
                self._make_mount_item(
                    mountpoint=None,
                    target=mnt_target,
                    mount_config=mount_config,
                ).mountpoint,
                "potato",
            )

    def _check_subvol_mounts_meow(self, subvol: Subvol) -> None:
        mount = [
            "(Dir)",
            {
                "meow": [
                    "(Dir)",
                    {
                        "MOUNT": [
                            "(Dir)",
                            {
                                "is_directory": ["(File d2)"],
                                "build_source": [
                                    "(Dir)",
                                    {
                                        "type": ["(File d6)"],
                                        "source": [f'(File d{len("//fake:path") + 1})'],
                                    },
                                ],
                                "runtime_source": [
                                    "(Dir)",
                                    {
                                        "so": ["(File d3)"],
                                        "arbitrary": [
                                            "(Dir)",
                                            {"j": ["(File d4)"]},
                                        ],
                                    },
                                ],
                            },
                        ]
                    },
                ]
            },
        ]
        expected_subvol: List[Union[Dict[str, Any], str]] = [
            "(Dir)",
            {
                "meow": ["(Dir)", {}],
                ".meta": get_meta_dir_contents(subvol),
            },
        ]
        # pyre-fixme[6]: In call `dict.__setitem__`, for 2nd positional only
        # parameter expected `Union[Dict[str, str], bool]` but got `str`.
        # pyre-ignore[16]: `str` has no attribute `__setitem__`.
        expected_subvol[1][".meta"][1]["private"][1]["mount"] = mount
        self.assertEqual(
            expected_subvol,
            render_subvol(subvol),
        )
        for filename, contents in (
            ("is_directory", "1\n"),
            ("build_source/type", "layer\n"),
            ("build_source/source", "//fake:path\n"),
            ("runtime_source/so", "me\n"),
            ("runtime_source/arbitrary/j", "son\n"),
        ):
            self.assertEqual(
                contents,
                subvol.read_path_text(
                    Path(".meta/private/mount/meow/MOUNT") / filename
                ),
            )

    def _write_layer_json_into(self, subvol: Subvol, out_dir: str) -> Path:
        subvol_path = subvol.path()
        # subvolumes_dir is the grandparent of the subvol by convention
        subvolumes_dir = subvol_path.dirname().dirname()
        with open(os.path.join(out_dir, "layer.json"), "w") as f:
            SubvolumeOnDisk.from_subvolume_path(
                subvol_path, subvolumes_dir
            ).to_json_file(f)
        return subvolumes_dir

    @with_mocked_temp_volume_dir
    def test_mount_item(self) -> None:
        with TempSubvolumes() as temp_subvolumes, tempfile.TemporaryDirectory() as source_dir:  # noqa: E501
            runtime_source = {"so": "me", "arbitrary": {"j": "son"}}
            mount_config = {
                "is_directory": True,
                "build_source": {"type": "layer", "source": "//fake:path"},
                "runtime_source": runtime_source,
            }
            with open(os.path.join(source_dir, "mountconfig.json"), "w") as f:
                json.dump(mount_config, f)
            self._check_item(
                self._make_mount_item(
                    mountpoint="can/haz",
                    target=source_dir,
                    mount_config=mount_config,
                ),
                {ProvidesDoNotAccess(path=Path("can/haz"))},
                {RequireDirectory(path=Path("can"))},
            )

            # Make a subvolume that would be mounted inside `mounter`
            mountee = temp_subvolumes.create("moun:tee/volume")

            # Make the JSON file normally in "buck-out" that refers to `mountee`
            mountee_subvolumes_dir = self._write_layer_json_into(mountee, source_dir)

            # Create a Mount <mountee> at <mounter>/meow
            mounter = temp_subvolumes.caller_will_create("mount:er/volume")
            root_item = FilesystemRootItem(from_target="t")
            root_item.get_phase_builder([root_item], DUMMY_LAYER_OPTS)(mounter)
            mount_meow = self._make_mount_item(
                mountpoint="meow", target=source_dir, mount_config=mount_config
            )
            self.assertEqual(runtime_source, json.loads(mount_meow.runtime_source))
            with self.assertRaisesRegex(AssertionError, " could not resolve "):
                mount_meow.build_source.to_path(
                    target_to_path={}, subvolumes_dir=mountee_subvolumes_dir
                )

            # Build will insert the proper metadata into the subvolume and
            # make sure the mountpoint exists, but it will not actually
            # do the mount itself.
            mount_meow.build(
                mounter,
                DUMMY_LAYER_OPTS._replace(
                    target_to_path={"//fake:path": source_dir},
                    subvolumes_dir=mountee_subvolumes_dir,
                ),
            )

            # This checks the subvolume's metadata contents for the mount
            self._check_subvol_mounts_meow(mounter)

            # Check that we read back the `mounter` metadata, mark `/meow`
            # inaccessible, and do not emit a `ProvidesFile` for `kitteh`.
            self._check_item(
                PhasesProvideItem(from_target="t", subvol=mounter),
                {
                    ProvidesDirectory(path=Path("/")),
                    ProvidesDoNotAccess(path=Path("/.meta")),
                    ProvidesDoNotAccess(path=Path("/meow")),
                    ProvidesUser("root"),
                    ProvidesGroup("root"),
                },
                set(),
            )
            # Check that we successfully clone mounts from the parent layer.
            mounter_child = temp_subvolumes.caller_will_create("child/volume")
            ParentLayerItem.get_phase_builder(
                [ParentLayerItem(from_target="t", subvol=mounter)],
                DUMMY_LAYER_OPTS,
            )(mounter_child)

            # The child has the same mount, and the same metadata
            self._check_subvol_mounts_meow(mounter_child)

    def test_parse_mount_meta(self) -> None:
        test_subvol = Subvol("small-layer-with-mounts", already_exists=True)

        expected_mounts = [
            Mount(
                build_source=BuildSource(
                    type="layer",
                    source=antlir_dep("compiler/test_images:hello_world_base"),
                ),
                is_directory=True,
                mountpoint="meownt",
                runtime_source=RuntimeSource(type="chicken", package=None, uuid=None),
            ),
            Mount(
                build_source=BuildSource(type="host", source="/dev/null"),
                is_directory=False,
                mountpoint="dev_null",
                runtime_source=None,
            ),
            Mount(
                build_source=BuildSource(type="host", source="/etc"),
                is_directory=True,
                mountpoint="host_etc",
                runtime_source=None,
            ),
        ]

        # Our test layer uses the build appliance as it's root, it might contain
        # more mounts than we are explicitly adding in our test cases.  Lets
        # just confirm that the mounts we expect are there.
        self.assertTrue(
            set(expected_mounts).issubset(set(mounts_from_meta(test_subvol.path())))
        )

        # Test a layer that has no mounts
        test_subvol_no_mounts = Subvol(
            "test-layer-without-mounts",
            already_exists=True,
        )

        self.assertEqual([], list(mounts_from_meta(test_subvol_no_mounts.path())))

        # Test when the path doesn't have a meta dir
        with temp_dir() as td:
            self.assertEqual([], list(mounts_from_meta(td)))
