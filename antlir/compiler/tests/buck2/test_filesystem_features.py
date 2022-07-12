#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import json
import subprocess
import unittest
from contextlib import contextmanager

from antlir.compiler.tests.buck2.helpers import (
    get_layer_target_to_path_by_prefix,
)
from antlir.config import antlir_dep
from antlir.find_built_subvol import find_built_subvol
from antlir.tests.subvol_helpers import pop_path, render_subvol

TARGET_TO_PATH = get_layer_target_to_path_by_prefix(
    importlib.resources.contents(__package__), __package__, "test_feature_"
)


class ImageFeatureTestCase(unittest.TestCase):
    test_install_dir = [
        "(Dir)",
        {
            "bar": [
                "(Dir)",
                {
                    "baz": ["(Dir)", {}],
                    "hello_world.tar": ["(File m444 d10240)"],
                    "hello_world_again.tar": ["(File m444 d10240)"],
                    "installed": [
                        "(Dir)",
                        {
                            "script-dir": [
                                "(Dir)",
                                {
                                    "data.txt": ["(File m444 d6)"],
                                    "subdir": [
                                        "(Dir)",
                                        {"exe.sh": ["(File m555 d21)"]},
                                    ],
                                },
                            ],
                            "solo-exe.sh": ["(File m555 d21)"],
                            "yittal-kitteh": ["(File m444 d5)"],
                        },
                    ],
                },
            ]
        },
    ]

    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    @contextmanager
    def target_subvol(self, target, mount_config=None):
        with self.subTest(target):
            # The mount configuration is very uniform, so we can check it here.
            expected_config = {
                "is_directory": True,
                "build_source": {
                    "type": "layer",
                    "source": antlir_dep(
                        "compiler/test_images/buck2/filesystem:" + target
                    ),
                },
            }
            if mount_config:
                expected_config.update(mount_config)
            with open(TARGET_TO_PATH[target] + "/mountconfig.json") as infile:
                self.assertEqual(expected_config, json.load(infile))
            yield find_built_subvol(TARGET_TO_PATH[target])

    def test_ensure_dirs_exist(self):
        with self.target_subvol("ensure-dirs-exist-layer") as subvol:
            rendered_subvol = render_subvol(subvol)

        ensure_dirs_exist_dir = pop_path(rendered_subvol, "ensure_dirs_exist")
        test_ensure_dirs_exist_dir = [
            "(Dir)",
            {
                "a": ["(Dir)", {}],
                "b": ["(Dir)", {"c": ["(Dir)", {}]}],
            },
        ]
        self.assertEquals(ensure_dirs_exist_dir, test_ensure_dirs_exist_dir)

    def test_ensure_subdirs_exist(self):
        with self.target_subvol("ensure-subdirs-exist-layer") as subvol:
            rendered_subvol = render_subvol(subvol)

        remove_dir = pop_path(rendered_subvol, "remove")
        test_remove_dir = [
            "(Dir)",
            {
                "a": ["(Dir)", {"b": ["(Dir)", {"c": ["(Dir)", {}]}]}],
                "b": ["(Dir)", {"c": ["(Dir)", {"d": ["(Dir)", {}]}]}],
                "c": ["(Dir)", {}],
            },
        ]
        self.assertEquals(remove_dir, test_remove_dir)

        ensure_subdirs_exist_dir = pop_path(
            rendered_subvol, "ensure_subdirs_exist"
        )
        test_ensure_subdirs_exist_dir = [
            "(Dir)",
            {
                "foo": [
                    "(Dir m555 o1111:1234)",
                    {
                        "bar": [
                            "(Dir m555 o1111:1234)",
                            {"baz": ["(Dir m555 o1111:1234)", {}]},
                        ]
                    },
                ]
            },
        ]
        self.assertEquals(
            ensure_subdirs_exist_dir, test_ensure_subdirs_exist_dir
        )

    def test_remove(self):
        with self.target_subvol("remove-layer") as subvol:
            rendered_subvol = render_subvol(subvol)

        remove_dir = pop_path(rendered_subvol, "remove")
        test_remove_dir = [
            "(Dir)",
            {
                "a": ["(Dir)", {"b": ["(Dir)", {}]}],
                "b": ["(Dir)", {}],
                "c": ["(Dir)", {}],
            },
        ]
        self.assertEquals(remove_dir, test_remove_dir)

    def test_symlink(self):
        with self.target_subvol("symlink-layer") as subvol:
            subvol_path = subvol.path()
            rendered_subvol = render_subvol(subvol)

        symlink_dir = pop_path(rendered_subvol, "symlink")
        test_symlink_dir = [
            "(Dir)",
            {
                "test_symlink_dir": ["(Symlink ../foo/bar)"],
                "test_symlink_file": ["(Symlink ../foo/bar/test_symlink.txt)"],
            },
        ]
        self.assertEquals(symlink_dir, test_symlink_dir)

        for path in [
            b"symlink/test_symlink_dir/test_symlink.txt",
            b"symlink/test_symlink_file",
        ]:
            self.assertEqual("symlink\n", (subvol_path / path).read_text())

    def test_install(self):
        with self.target_subvol("install-files-layer") as subvol:
            rendered_subvol = render_subvol(subvol)

        install_dir = pop_path(rendered_subvol, "install")
        self.assertEquals(install_dir, self.test_install_dir)

    def test_install_buck_runnable(self):
        with self.target_subvol("install-buck-runnable-layer") as subvol:
            subvol_path = subvol.path()
            rendered_subvol = render_subvol(subvol)

        install_buck_runnable_dir = set(pop_path(rendered_subvol, "install")[1])
        test_install_buck_runnable_dir = {"print-ok", "print-ok-too"}
        self.assertEquals(
            install_buck_runnable_dir, test_install_buck_runnable_dir
        )

        self.assertEquals(
            subprocess.check_output(str(subvol_path) + "/install/print-ok"),
            b"ok\n",
        )

    def test_tarball(self):
        with self.target_subvol("tarball-layer") as subvol:
            rendered_subvol = render_subvol(subvol)

        tarball_dir = pop_path(rendered_subvol, "tarball")
        test_tarball_dir = [
            "(Dir)",
            {
                "foo": [
                    "(Dir)",
                    {
                        "borf": [
                            "(Dir)",
                            {
                                "barf": ["(Dir)", {"hello_world": ["(File)"]}],
                                "hello_world": ["(File)"],
                            },
                        ],
                        "hello_world": ["(File)"],
                    },
                ]
            },
        ]
        self.assertEquals(tarball_dir, test_tarball_dir)

    def test_clone(self):
        with self.target_subvol("clone-layer") as subvol:
            rendered_subvol = render_subvol(subvol)

        clone_dir = pop_path(rendered_subvol, "clone")
        test_clone_dir = [
            "(Dir)",
            {
                "case1": [
                    "(Dir)",
                    {"install": self.test_install_dir},
                ],
                "case2": self.test_install_dir,
                "case3": self.test_install_dir,
            },
        ]
        self.assertEquals(clone_dir, test_clone_dir)
