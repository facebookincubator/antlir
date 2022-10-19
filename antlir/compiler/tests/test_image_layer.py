#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import json
import os
import unittest
from contextlib import contextmanager

from antlir.artifacts_dir import find_repo_root
from antlir.btrfs_diff.tests.demo_sendstreams_expected import (
    render_demo_subvols,
)
from antlir.compiler.items.meta_key_value_store import (
    load_meta_key_value_store_items,
    MetaKeyValueStoreItem,
)
from antlir.compiler.items.mount import mounts_from_meta

from antlir.compiler.procfs_serde import deserialize_int
from antlir.config import antlir_dep, repo_config
from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import Path
from antlir.tests.flavor_helpers import (
    get_rpm_installers_supported,
    render_flavor,
)
from antlir.tests.layer_resource import layer_resource, LAYER_SLASH_ENCODE
from antlir.tests.subvol_helpers import (
    check_common_rpm_render,
    get_meta_dir_contents,
    pop_path,
    render_meta_build_contents,
    render_subvol,
)


TARGET_RESOURCE_PREFIX = "test_image_layer_path_to_"
TARGET_TO_PATH = {
    target[len(TARGET_RESOURCE_PREFIX) :]: path
    for target, path in [
        (
            target.replace(LAYER_SLASH_ENCODE, "/"),
            str(layer_resource(__package__, target)),
        )
        for target in importlib.resources.contents(__package__)
        if target.startswith(TARGET_RESOURCE_PREFIX)
    ]
}


REPO_CFG = repo_config()
SHADOW_ME = "shadow me"


class ImageLayerTestCase(unittest.TestCase):
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
                    "source": antlir_dep("compiler/test_images:" + target),
                },
            }
            if mount_config:
                expected_config.update(mount_config)
            with open(TARGET_TO_PATH[target] + "/mountconfig.json") as infile:
                self.assertEqual(expected_config, json.load(infile))
            yield find_built_subvol(TARGET_TO_PATH[target])

    def _check_hello(self, subvol_path):
        self.assertEqual("", (subvol_path / "hello_world").read_text())

    def _check_parent(self, subvol):
        subvol_path = subvol.path()
        self._check_hello(subvol_path)
        # :parent_layer
        for path in [
            b"rpm_test/hello_world.tar",
            b"foo/bar/even_more_hello_world.tar",
        ]:
            self.assertTrue(
                os.path.isfile(os.path.join(subvol_path, path)), path
            )

        # :feature_dirs not tested by :parent_layer
        self.assertTrue(
            os.path.isdir(os.path.join(subvol_path, b"foo/bar/baz"))
        )
        self.assertTrue(os.path.isdir(os.path.join(subvol_path, b"alpha/beta")))

        # :hello_world_base has a mount entry in the meta.  Note that this
        # *does not* validate that the mount itself exists.
        self.assertTrue(
            "mounted_hello"
            in (m.mountpoint for m in mounts_from_meta(subvol.path()))
        )

        # :feature_symlinks
        for source, dest in [
            (b"bar", b"foo/fighter"),
            (b"bar", b"foo/face"),
            (b"..", b"foo/bar/baz/bar"),
            (b"hello_world.tar", b"foo/symlink_to_hello_world.tar"),
        ]:
            self.assertTrue(
                os.path.exists(
                    os.path.join(subvol_path, os.path.dirname(dest), source)
                ),
                (dest, source),
            )
            self.assertTrue(
                os.path.islink(os.path.join(subvol_path, dest)), dest
            )
            self.assertEqual(
                source, os.readlink(os.path.join(subvol_path, dest))
            )

    def _check_child(self, subvol):
        subvol_path = subvol.path()
        self._check_parent(subvol)
        for path in [
            # :feature_tar_and_rpms
            b"foo/borf/hello_world",
            b"foo/hello_world",
            b"rpm_test/mice.txt",
            b"rpm_test/cheese2.txt",
            # :child/layer
            b"foo/extracted_hello/hello_world",
            b"foo/more_extracted_hello/hello_world",
        ]:
            self.assertTrue(os.path.isfile(os.path.join(subvol_path, path)))
        for path in [
            # :feature_tar_and_rpms ensures these are absent
            b"rpm_test/carrot.txt",
            b"rpm_test/milk.txt",
        ]:
            self.assertFalse(os.path.exists(os.path.join(subvol_path, path)))

    def test_image_layer_targets(self):
        # Future: replace these checks by a more comprehensive test of the
        # image's data & metadata using our `btrfs_diff` library.
        with self.target_subvol(
            "hello_world_base",
            mount_config={"runtime_source": {"type": "chicken"}},
        ) as subvol:
            self._check_hello(subvol.path())
        with self.target_subvol(
            "parent_layer", mount_config={"runtime_source": {"type": "turkey"}}
        ) as subvol:
            self._check_parent(subvol)
            # Cannot check this in `_check_parent`, since that gets called
            # by `_check_child`, but the RPM gets removed in the child.
            self.assertTrue(os.path.isfile(subvol.path("rpm_test/carrot.txt")))
        with self.target_subvol("child/layer") as subvol:
            self._check_child(subvol)
        with self.target_subvol("base_cheese_layer") as subvol:
            self.assertTrue(
                os.path.isfile(subvol.path("/rpm_test/cheese2.txt"))
            )
        with self.target_subvol("older_cheese_layer") as subvol:
            self.assertTrue(
                os.path.isfile(subvol.path("/rpm_test/cheese1.txt"))
            )
            # Make sure the original file is removed when the RPM is downgraded
            self.assertFalse(
                os.path.isfile(subvol.path("/rpm_test/cheese2.txt"))
            )
        with self.target_subvol("newer_cheese_layer") as subvol:
            self.assertTrue(
                os.path.isfile(subvol.path("/rpm_test/cheese3.txt"))
            )
            # Make sure the original file is removed when the RPM is upgraded
            self.assertFalse(
                os.path.isfile(subvol.path("/rpm_test/cheese2.txt"))
            )
        with self.target_subvol("reinstall_cheese_layer") as subvol:
            self.assertTrue(
                os.path.isfile(subvol.path("/rpm_test/cheese2.txt"))
            )

    def test_layer_from_demo_sendstreams(self):
        # `btrfs_diff.demo_sendstream` produces a subvolume send-stream with
        # fairly thorough coverage of filesystem features.  This test grabs
        # that send-stream, receives it into an `image_layer`, and validates
        # that the send-stream of the **received** volume has the same
        # rendering as the original send-stream was supposed to have.
        #
        # In other words, besides testing `image_sendstream_layer`, this is
        # also a test of idempotence for btrfs send+receive.
        #
        # Notes:
        #  - `compiler/tests/TARGETS` explains why `mutate_ops` is not here.
        #  - Currently, `mutate_ops` also uses `--no-data`, which would
        #    break this test of idempotence.
        for original_name, subvol_name, mount_config in [
            ("volume", "create_ops", None),
            ("volume", "create_ops-from-dir", None),
            ("volume", "create_ops-from-layer", None),
            (
                "volume",
                "create_ops-alias",
                {
                    "build_source": {
                        "type": "layer",
                        "source": antlir_dep("compiler/test_images:create_ops"),
                    }
                },
            ),
        ]:
            with self.target_subvol(
                subvol_name, mount_config=mount_config
            ) as sv:
                rendered_subvol = render_subvol(sv)
                self.assertEqual(
                    get_meta_dir_contents(subvol=sv),
                    pop_path(rendered_subvol, ".meta"),
                )
                self.assertEqual(
                    render_demo_subvols(create_ops=original_name),
                    rendered_subvol,
                )

    # This is reused by `test_genrule_layer` because we currently lack
    # rendering for incremental sendstreams.
    @contextmanager
    def _check_build_appliance(self, rsrc_name, yum_dnf):
        with self.target_subvol(rsrc_name) as sv:
            r = render_subvol(sv)
            (ino,) = pop_path(r, "bin/sh")  # Busybox from `rpm-test-milk`
            # NB: We changed permissions on this at some point, but after
            # the migration diffs land, the [75] can become a 5.
            self.assertRegex(ino, r"^\(File m[75]55 d[0-9]+\)$")

            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "milk.txt": ["(File d12)"],
                        # From the `rpm-test-milk` post-install script
                        "post.txt": ["(File d6)"],
                    },
                ],
                pop_path(r, "rpm_test"),
            )

            ino, _ = pop_path(r, "usr/lib/.build-id")
            self.assertEqual("(Dir)", ino)
            self.assertEqual(["(Dir)", {}], pop_path(r, "bin"))

            yield sv, r

            self.assertEqual(["(Dir)", {}], pop_path(r, "var/tmp"))
            self.assertEqual(["(Dir)", {}], pop_path(r, "usr"))

            # This is never changed in the underlying layer, just shadowed
            # at runtime.
            self.assertEqual(
                [f"(File m444 d{len(SHADOW_ME)})"],
                pop_path(r, "shadow_me"),
            )

            check_common_rpm_render(self, r, yum_dnf, subvol=sv)

    @unittest.skipUnless(
        "dnf" in get_rpm_installers_supported(),
        "dnf not supported",
    )
    def test_dnf_build_appliance(self):
        with self._check_build_appliance(
            "validates-dnf-build-appliance", "dnf"
        ) as (_, r):
            self.assertEqual(["(Dir)", {}], pop_path(r, "usr/lib"))

    @unittest.skipUnless(
        "yum" in get_rpm_installers_supported(),
        "yum not supported",
    )
    def test_yum_build_appliance(self):
        with self._check_build_appliance(
            "validates-yum-build-appliance", "yum"
        ) as (_, r):
            self.assertEqual(["(Dir)", {}], pop_path(r, "usr/lib"))

    def test_genrule_layer(self):
        # This checks that `shadow_paths` worked as expected.
        shadowed = "milk 2.71 8\n"
        assert len(shadowed) != len(SHADOW_ME), (shadowed, SHADOW_ME)
        with self._check_build_appliance("genrule-layer", "dnf") as (sv, r):
            # The desired side effect of the run.  This also checks that the
            # path shadowing worked.
            self.assertEqual(
                [f"(File d{len(shadowed)})"],
                pop_path(r, "I_AM_GENRULE_LAYER"),
            )

            # Fixme: This `os-release` is an artifact of `nspawn_in_subvol`.
            # We should probably not be leaking this into the layer, but
            # it's unlikely to show up in real-world examples.
            self.assertEqual(
                ["(Dir)", {"os-release": ["(File)"]}], pop_path(r, "usr/lib")
            )

            # Maybe fixme: `nspawn_in_subvol` could potentially clean this
            # up but it seems unlikely to affect prod since it's only a
            # thing in `@mode/dev`, which should never ship prod artifacts.
            if deserialize_int(
                sv.path(), "/.meta/private/opts/artifacts_may_require_repo"
            ):
                # Assume that the prefix of the repo (e.g. /home or /data)
                # is not one of the normal FHS-type directories below.
                repo = find_repo_root().abspath()
                top = repo
                while True:
                    parent = top.dirname()
                    if parent == b"/":
                        break
                    top = parent

                # Sometimes the artifacts dirs are under the same top-level
                # directory, so we expect any of those and the repo to be
                # present
                paths_to_verify = [repo]
                expected = {}
                for d in REPO_CFG.host_mounts_for_repo_artifacts:
                    d = d.abspath()
                    if d.startswith(top):
                        paths_to_verify.append(d)
                    else:
                        # pop any others so they don't affect future comparisons
                        # I know this will make someone very frustrated again
                        # in the future, but it's just too crazy to account for
                        # all of the many cases in which this can break
                        while d != b"/":
                            pop_path(r, str(d))
                            d = d.dirname()

                for d in paths_to_verify:
                    while d != b"/":
                        nested = expected
                        for part in str(d).split(os.path.sep)[2:]:
                            if part not in nested:
                                nested[part] = ["(Dir)", {}]
                            nested = nested[part][1]
                        d = d.dirname()

                self.assertEqual(["(Dir)", expected], pop_path(r, str(top)))

            # Clean other, less sketchy side effects of `nspawn_in_subvol`:
            # empty LFS directories.
            for d in ("proc", "run", "sys", "tmp"):
                self.assertEqual(["(Dir)", {}], pop_path(r, d))
            # ignore /root if it is not present - when the repo is present and
            # under /root it will be processed above
            self.assertEqual(
                ["(Dir m750)", {}], pop_path(r, "/root", ["(Dir m750)", {}])
            )

            # This nspawn-created symlink isn't great, but, again, it
            # shouldn't affect production use-cases.
            self.assertEqual(["(Symlink usr/lib)"], pop_path(r, "lib"))

    def test_genrule_layer_mounts(self):
        # test_genrule_layer checks the actual image contents
        with self.target_subvol("genrule-layer-with-mounts") as sv:
            # Check that the `layer_mount` was mounted when the genrule
            # layer ran
            mounts = sv.read_path_text(Path("/GENRULE_LAYER_MOUNTS"))
            self.assertIn("/meownt", mounts)
            self.assertIn("/sendstream_meownt", mounts)

    def test_non_default_rpm_snapshot(self):
        with self.target_subvol("layer-with-non-default-snapshot-rpm") as sv:
            r = render_subvol(sv)

            self.assertEqual(
                [
                    "(Dir)",
                    {"cake.txt": ["(File d17)"], "cheese.txt": ["(File d11)"]},
                ],
                pop_path(r, "rpm_test"),
            )

            check_common_rpm_render(
                self,
                r,
                REPO_CFG.flavor_to_config["centos7"].rpm_installer,
                subvol=sv,
            )

    def _check_installed_files_bar(self, r, clones_re=""):
        # We don't know the exact sizes because these may be wrapped
        (ino,) = pop_path(r, "installed/print-arg")
        self.assertRegex(ino, r"^\(File m555 d[0-9]+" + clones_re + r"\)$")
        (ino,) = pop_path(r, "installed/print-ok")
        self.assertRegex(ino, r"^\(File m555 d[0-9]+" + clones_re + r"\)$")
        (ino,) = pop_path(r, "installed/print-ok-too")
        self.assertRegex(ino, r"^\(File m555 d[0-9]+" + clones_re + r"\)$")
        (ino,) = pop_path(r, "installed/print-ok-again")
        self.assertRegex(ino, r"^\(File m555 d[0-9]+" + clones_re + r"\)$")
        (hello_ino,) = pop_path(r, "hello_world.tar")
        # Depending on the build host OS, our tarball may or may not get
        # automatically sparsified.
        for hello_suffix in ["d10240)", "d4096h6144)"]:
            if hello_ino.endswith(hello_suffix):
                break
        else:
            raise AssertionError(f"Bad hello_world.tar: {hello_ino}")
        self.assertEqual(f"(File m444 {hello_suffix}", hello_ino)

        self.assertEqual(
            [
                "(Dir)",
                {
                    "baz": ["(Dir)", {}],
                    "hello_world_again.tar": [f"(File m444 {hello_suffix}"],
                    "installed": [
                        "(Dir)",
                        {
                            "yittal-kitteh": ["(File m444 d5)"],
                            "script-dir": [
                                "(Dir)",
                                {
                                    "subdir": [
                                        "(Dir)",
                                        {"exe.sh": ["(File m555 d21)"]},
                                    ],
                                    "data.txt": ["(File m444 d6)"],
                                },
                            ],
                            "solo-exe.sh": ["(File m555 d21)"],
                        },
                    ],
                },
            ],
            r,
        )

    def test_installed_files(self):
        with self.target_subvol("installed-files") as sv:
            r = render_subvol(sv)
            pop_path(r, "alpha")
            self._check_installed_files_bar(pop_path(r, "foo/bar"))
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        "foo": ["(Dir)", {}],
                        ".meta": [
                            "(Dir)",
                            {
                                "build": render_meta_build_contents(sv),
                                "flavor": [render_flavor(flavor="antlir_test")],
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
                                        ]
                                    },
                                ],
                            },
                        ],
                    },
                ],
                r,
            )

    def test_cloned_files(self):
        with self.target_subvol("cloned-files") as sv:
            r = render_subvol(sv)
            for bar in ["bar", "bar2", "bar3"]:
                self._check_installed_files_bar(
                    pop_path(r, bar),
                    # When multiple `test_cloned_files` run concurrently,
                    # this would render cross-subvolume clones.  This is
                    # expected, but super-annoying to test tightly.
                    clones_re=r"(|\([^)]*\))",
                )
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        ".meta": [
                            "(Dir)",
                            {
                                "build": render_meta_build_contents(sv),
                                "flavor": [render_flavor(flavor="antlir_test")],
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
                                        ]
                                    },
                                ],
                            },
                        ]
                    },
                ],
                r,
            )

    def test_meta_store(self):
        with self.target_subvol("meta_store_layer") as sv:
            r = render_subvol(sv)
            self.assertEqual(
                [
                    "(Dir)",
                    {
                        ".meta": [
                            "(Dir)",
                            {
                                "build": render_meta_build_contents(sv),
                                "flavor": [render_flavor(flavor="antlir_test")],
                                "key_value_store": ["(File d189)"],
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
                                        ]
                                    },
                                ],
                            },
                        ]
                    },
                ],
                r,
            )

            items = load_meta_key_value_store_items(sv)
            self.assertEqual(
                items,
                [
                    MetaKeyValueStoreItem(key="key1", value="value1"),
                    MetaKeyValueStoreItem(
                        key="key2", value="value2", require_keys=["key1"]
                    ),
                ],
            )

    def test_remove_meta_store(self):
        with self.target_subvol("remove_meta_store_layer") as sv:
            items = load_meta_key_value_store_items(sv)
            self.assertEqual(
                items,
                [
                    MetaKeyValueStoreItem(key="key1", value="value1"),
                ],
            )
