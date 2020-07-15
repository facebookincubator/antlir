#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import sys
import tempfile

from fs_image.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesDoNotAccess,
    require_directory,
)
from fs_image.compiler.subvolume_on_disk import SubvolumeOnDisk
from fs_image.tests.layer_resource import layer_resource_subvol
from fs_image.tests.temp_subvolumes import TempSubvolumes

from ..make_subvol import FilesystemRootItem, ParentLayerItem
from ..mount import (
    BuildSource,
    Mount,
    MountItem,
    RuntimeSource,
    mounts_from_subvol_meta,
)
from ..phases_provide import PhasesProvideItem
from .common import DUMMY_LAYER_OPTS, BaseItemTestCase, render_subvol


def _mount_item_new(from_target, mount_config):
    return MountItem(
        layer_opts=DUMMY_LAYER_OPTS._replace(
            allowed_host_mount_targets=["//dummy/host_mounts:t"]
        ),
        from_target=from_target,
        mountpoint="/lala",
        target=None,
        mount_config=mount_config,
    )


class MountItemTestCase(BaseItemTestCase):
    def test_mount_item_file_from_host(self):
        mount_config = {
            "is_directory": False,
            "build_source": {"type": "host", "source": "/dev/null"},
        }

        with self.assertRaisesRegex(AssertionError, "must be located under"):
            _mount_item_new("t", mount_config)

        bad_mount_config = mount_config.copy()
        bad_mount_config["runtime_source"] = bad_mount_config["build_source"]
        with self.assertRaisesRegex(AssertionError, "Only `build_source` may "):
            _mount_item_new("//dummy/host_mounts:t", bad_mount_config)

        mount_item = _mount_item_new("//dummy/host_mounts:t", mount_config)

        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create("mounter")
            mount_item.build(
                subvol,
                DUMMY_LAYER_OPTS._replace(
                    target_to_path={}, subvolumes_dir="unused"
                ),
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
                                            "source": [
                                                f'(File d{len("/dev/null")+1})'
                                            ],
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
                        "meta": [
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
                with open(
                    subvol.path(
                        os.path.join("meta/private/mount/lala/MOUNT", filename)
                    )
                ) as f:
                    self.assertEqual(contents, f.read())

    def _make_mount_item(
        self, *, mountpoint, target, mount_config, from_target="t"
    ):
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

    def test_mount_item_default_mountpoint(self):
        with tempfile.TemporaryDirectory() as mnt_target:
            mount_config = {
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

    def _check_subvol_mounts_meow(self, subvol):
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
                                        "source": [
                                            f'(File d{len("//fake:path") + 1})'
                                        ],
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
        self.assertEqual(
            [
                "(Dir)",
                {
                    "meow": ["(Dir)", {}],
                    "meta": [
                        "(Dir)",
                        {
                            "private": [
                                "(Dir)",
                                {
                                    "opts": [
                                        "(Dir)",
                                        {
                                            "artifacts_may_require_repo": [
                                                "(File d2)"
                                            ]
                                        },
                                    ],
                                    "mount": mount,
                                },
                            ]
                        },
                    ],
                },
            ],
            render_subvol(subvol),
        )
        for filename, contents in (
            ("is_directory", "1\n"),
            ("build_source/type", "layer\n"),
            ("build_source/source", "//fake:path\n"),
            ("runtime_source/so", "me\n"),
            ("runtime_source/arbitrary/j", "son\n"),
        ):
            with open(
                subvol.path(
                    os.path.join("meta/private/mount/meow/MOUNT", filename)
                )
            ) as f:
                self.assertEqual(contents, f.read())

    def _write_layer_json_into(self, subvol, out_dir):
        subvol_path = subvol.path().decode()
        # subvolumes_dir is the grandparent of the subvol by convention
        subvolumes_dir = os.path.dirname(os.path.dirname(subvol_path))
        with open(os.path.join(out_dir, "layer.json"), "w") as f:
            SubvolumeOnDisk.from_subvolume_path(
                subvol_path, subvolumes_dir
            ).to_json_file(f)
        return subvolumes_dir

    def test_mount_item(self):
        with TempSubvolumes(
            sys.argv[0]
        ) as temp_subvolumes, tempfile.TemporaryDirectory() as source_dir:
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
                {ProvidesDoNotAccess(path="can/haz")},
                {require_directory("can")},
            )

            # Make a subvolume that we will mount inside `mounter`
            mountee = temp_subvolumes.create("moun:tee/volume")
            mountee.run_as_root(["tee", mountee.path("kitteh")], input=b"cheez")

            # These sub-mounts inside `mountee` act as canaries to make sure
            # that (a) `mounter` receives the sub-mounts as a consequence of
            # mounting `mountee` recursively, (b) that unmounting one in
            # `mounter` does not affect the original in `mountee` -- i.e.
            # that rslave propagation is set up correctly, (c) that
            # unmounting in `mountee` immediately affects `mounter`.
            #
            # In practice, our build artifacts should NEVER be mutated after
            # construction (and the only un-mount is implicitly, and
            # seemingly safely, performed by `btrfs subvolume delete`).
            # However, ensuring that we have correct `rslave` propagation is
            # a worthwhile safeguard for host mounts, where an errant
            # `umount` by a user inside their repo could otherwise break
            # their host.
            for submount in ("submount1", "submount2"):
                mountee.run_as_root(["mkdir", mountee.path(submount)])
                mountee.run_as_root(
                    [
                        "mount",
                        "-o",
                        "bind,ro",
                        source_dir,
                        mountee.path(submount),
                    ]
                )
                self.assertTrue(
                    os.path.exists(mountee.path(submount + "/mountconfig.json"))
                )

            # Make the JSON file normally in "buck-out" that refers to `mountee`
            mountee_subvolumes_dir = self._write_layer_json_into(
                mountee, source_dir
            )

            # Mount <mountee> at <mounter>/meow
            mounter = temp_subvolumes.caller_will_create("mount:er/volume")
            root_item = FilesystemRootItem(from_target="t")
            root_item.get_phase_builder([root_item], DUMMY_LAYER_OPTS)(mounter)
            mount_meow = self._make_mount_item(
                mountpoint="meow", target=source_dir, mount_config=mount_config
            )
            self.assertEqual(
                runtime_source, json.loads(mount_meow.runtime_source)
            )
            with self.assertRaisesRegex(AssertionError, " could not resolve "):
                mount_meow.build_source.to_path(
                    target_to_path={}, subvolumes_dir=mountee_subvolumes_dir
                )
            mount_meow.build(
                mounter,
                DUMMY_LAYER_OPTS._replace(
                    target_to_path={"//fake:path": source_dir},
                    subvolumes_dir=mountee_subvolumes_dir,
                ),
            )

            # This checks the subvolume **contents**, but not the mounts.
            # Ensure the build created a mountpoint, and populated metadata.
            self._check_subvol_mounts_meow(mounter)

            # `mountee` was also mounted at `/meow`
            with open(mounter.path("meow/kitteh")) as f:
                self.assertEqual("cheez", f.read())

            def check_mountee_mounter_submounts(submount_presence):
                for submount, (in_mountee, in_mounter) in submount_presence:
                    self.assertEqual(
                        in_mountee,
                        os.path.exists(
                            mountee.path(submount + "/mountconfig.json")
                        ),
                        f"{submount}, {in_mountee}",
                    )
                    self.assertEqual(
                        in_mounter,
                        os.path.exists(
                            mounter.path(
                                "meow/" + submount + "/mountconfig.json"
                            )
                        ),
                        f"{submount}, {in_mounter}",
                    )

            # Both sub-mounts are accessible in both places now.
            check_mountee_mounter_submounts(
                [("submount1", (True, True)), ("submount2", (True, True))]
            )
            # Unmounting `submount1` from `mountee` also affects `mounter`.
            mountee.run_as_root(["umount", mountee.path("submount1")])
            check_mountee_mounter_submounts(
                [("submount1", (False, False)), ("submount2", (True, True))]
            )
            # Unmounting `submount2` from `mounter` doesn't affect `mountee`.
            mounter.run_as_root(["umount", mounter.path("meow/submount2")])
            check_mountee_mounter_submounts(
                [("submount1", (False, False)), ("submount2", (True, False))]
            )

            # Check that we read back the `mounter` metadata, mark `/meow`
            # inaccessible, and do not emit a `ProvidesFile` for `kitteh`.
            self._check_item(
                PhasesProvideItem(from_target="t", subvol=mounter),
                {
                    ProvidesDirectory(path="/"),
                    ProvidesDoNotAccess(path="/meta"),
                    ProvidesDoNotAccess(path="/meow"),
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

            # Check that we refuse to create nested mounts.
            nested_mounter = temp_subvolumes.create("nested_mounter")
            nested_item = MountItem(
                layer_opts=DUMMY_LAYER_OPTS,
                from_target="t",
                mountpoint="/whatever",
                target=None,
                mount_config={
                    "is_directory": True,
                    "build_source": {"type": "layer", "source": "//:fake"},
                },
            )
            with tempfile.TemporaryDirectory() as d:
                mounter_subvolumes_dir = self._write_layer_json_into(mounter, d)
                with self.assertRaisesRegex(
                    AssertionError, "Refusing .* nested mount"
                ):
                    nested_item.build(
                        nested_mounter,
                        DUMMY_LAYER_OPTS._replace(
                            target_to_path={"//:fake": d},
                            subvolumes_dir=mounter_subvolumes_dir,
                        ),
                    )

    def test_parse_mount_meta(self):
        test_subvol = layer_resource_subvol(
            __package__, "test-layer-with-mounts"
        )

        self.assertEqual(
            sorted(
                [
                    Mount(
                        build_source=BuildSource(
                            type="layer",
                            source="//fs_image/compiler/test_images:"
                            + "hello_world_base",
                        ),
                        is_directory=True,
                        mountpoint="mounted_hello",
                        runtime_source=RuntimeSource(type="chicken"),
                    ),
                    Mount(
                        build_source=BuildSource(
                            type="host", source="/dev/null"
                        ),
                        is_directory=False,
                        mountpoint="dev_null",
                        runtime_source=None,
                    ),
                ],
                key=lambda m: m.mountpoint,
            ),
            sorted(
                mounts_from_subvol_meta(test_subvol), key=lambda m: m.mountpoint
            ),
        )

        # Test a layer that has no mounts
        test_subvol_no_mounts = layer_resource_subvol(
            __package__, "test-layer-without-mounts"
        )

        self.assertEqual(
            [], list(mounts_from_subvol_meta(test_subvol_no_mounts))
        )
