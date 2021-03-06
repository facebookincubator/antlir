#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import itertools
import json
import os
import subprocess
import sys
import tempfile
import unittest
import unittest.mock
from contextlib import contextmanager

from antlir import subvol_utils
from antlir.compiler.items import (
    ensure_dirs_exist,
    rpm_action,
    symlink,
    tarball,
)
from antlir.find_built_subvol import _get_subvolumes_dir
from antlir.fs_utils import Path, temp_dir
from antlir.nspawn_in_subvol import ba_runner
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.subvol_utils import TempSubvolumes
from antlir.tests.layer_resource import layer_resource, layer_resource_subvol

from .. import subvolume_on_disk as svod
from ..compiler import LayerOpts, build_image, parse_args
from . import sample_items as si


_orig_btrfs_get_volume_props = svod._btrfs_get_volume_props
# We need the actual subvolume directory for this mock because the
# `MountItem` build process in `test_compiler.py` loads a real subvolume
# through this path (`:hello_world_base`).
_SUBVOLS_DIR = _get_subvolumes_dir()
_FAKE_SUBVOL = Path("FAKE_SUBVOL")
_FIND_ARGS = [
    "find",
    "-P",
    _SUBVOLS_DIR / _FAKE_SUBVOL,
    "(",
    "-path",
    _SUBVOLS_DIR / _FAKE_SUBVOL / ".meta",
    ")",
    "-prune",
    "-o",
    "-printf",
    "%y %p\\0",
]


def _subvol_mock_lexists_is_btrfs_and_run_as_root(fn):
    """
    The purpose of these mocks is to run the compiler while recording
    what commands we WOULD HAVE run on the subvolume.  This is possible
    because all subvolume mutations are supposed to go through
    `Subvol.run_as_root`.  This lets our tests assert that the
    expected operations would have been executed.
    """
    fn = unittest.mock.patch.object(os.path, "lexists")(fn)
    fn = unittest.mock.patch.object(subvol_utils, "_path_is_btrfs_subvol")(fn)
    fn = unittest.mock.patch.object(subvol_utils.Subvol, "run_as_root")(fn)
    fn = unittest.mock.patch.object(rpm_action, "run_nspawn")(fn)
    fn = unittest.mock.patch.object(tarball, "run_nspawn")(fn)
    fn = unittest.mock.patch.object(symlink, "run_nspawn")(fn)
    fn = unittest.mock.patch.object(ensure_dirs_exist, "run_nspawn")(fn)
    fn = unittest.mock.patch.object(ensure_dirs_exist, "mode_to_octal_str")(fn)
    fn = unittest.mock.patch.object(ba_runner, "run_nspawn")(fn)
    return fn


def _run_as_root(args, **kwargs):
    """
    DependencyGraph adds a PhasesProvideItem to traverse the subvolume, as
    modified by the phases. This ensures the traversal produces a subvol /
    """
    if args[0] == "find":
        assert args == _FIND_ARGS, args
        ret = unittest.mock.Mock()
        ret.stdout = f"d {_SUBVOLS_DIR/_FAKE_SUBVOL}\0".encode()
        return ret


def _os_path_lexists(path):
    """
    This ugly mock exists because I don't want to set up a fake subvolume,
    from which the `sample_items` `RemovePathItem`s can remove their files.
    """
    if path.endswith(b"/to/remove"):
        return True
    assert "AFAIK, os.path.lexists is only used by the `RemovePathItem` tests"


def _btrfs_get_volume_props(subvol_path):
    if subvol_path == _SUBVOLS_DIR / _FAKE_SUBVOL:
        # We don't have an actual btrfs subvolume, so make up a UUID.
        return {"UUID": "fake uuid", "Parent UUID": None}
    return _orig_btrfs_get_volume_props(subvol_path)


@contextmanager
def mock_layer_dir_access(test_case, subvolume_path):
    """
    `SubvolumeOnDisk` does a ton of validation, which makes it hard to
    use it to read or write subvols that are not actual target outputs.

    Instead, this yields a fake layer directory path, and mocks
    `SubvolumeOnDisk.from_json_file` **only** for calls querying the fake
    path.  For those calls, it returns a fake `SubvolumeOnDisk` pointing at
    the supplied `subvolume_path`.
    """
    sigil_dirname = b"fake-parent-layer"
    orig_from_json_file = svod.SubvolumeOnDisk.from_json_file
    with unittest.mock.patch.object(
        svod.SubvolumeOnDisk, "from_json_file"
    ) as from_json_file, temp_dir() as td:
        parent_layer_file = td / sigil_dirname / "layer.json"
        os.mkdir(parent_layer_file.dirname())
        with open(parent_layer_file, "w") as f:
            f.write("this will never be read")

        def check_call(infile, subvolumes_dir):
            if Path(infile.name).dirname().basename() != sigil_dirname:
                return orig_from_json_file(infile, subvolumes_dir)

            test_case.assertEqual(parent_layer_file, infile.name)
            test_case.assertEqual(_SUBVOLS_DIR, subvolumes_dir)

            class FakeSubvolumeOnDisk:
                def subvolume_path(self):
                    return subvolume_path.decode()

            return FakeSubvolumeOnDisk()

        from_json_file.side_effect = check_call
        yield parent_layer_file.dirname()


class CompilerTestCase(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    @_subvol_mock_lexists_is_btrfs_and_run_as_root
    @unittest.mock.patch.object(svod, "_btrfs_get_volume_props")
    def _compile(
        self,
        args,
        btrfs_get_volume_props,
        lexists,
        is_btrfs,
        run_as_root,
        *_run_nspawns,
    ):
        lexists.side_effect = _os_path_lexists
        run_as_root.side_effect = _run_as_root
        btrfs_get_volume_props.side_effect = _btrfs_get_volume_props
        # Since we're not making subvolumes, we need this so that
        # `Subvolume(..., already_exists=True)` will work.
        is_btrfs.return_value = True
        return (
            build_image(
                parse_args(
                    [
                        # Must match LayerOpts below
                        "--artifacts-may-require-repo",
                        "--rpm-installer=dnf",
                        "--subvolumes-dir",
                        _SUBVOLS_DIR,
                        "--subvolume-rel-path",
                        _FAKE_SUBVOL,
                        "--build-appliance",
                        layer_resource(__package__, "test-build-appliance"),
                        "--child-layer-target",
                        "CHILD_TARGET",
                        "--child-feature-json",
                        si.TARGET_TO_PATH[si.mangle(si.T_KITCHEN_SINK)],
                    ]
                    + args
                )
            ),
            run_as_root.call_args_list,
        )

    def _compiler_run_as_root_calls(self, *, parent_feature_json, parent_dep):
        """
        Invoke the compiler on the targets from the "sample_items" test
        example, and ensure that the commands that the compiler would run
        are exactly the same ones that correspond to the expected
        `ImageItems`.

        In other words, these test assert that the compiler would run the
        right commands, without verifying their sequencing.  That is OK,
        since the dependency sort has its own unit test, and moreover
        `test_image_layer.py` does an end-to-end test that validates the
        final state of a compiled, live subvolume.
        """
        with tempfile.NamedTemporaryFile() as tf:
            deps = parent_dep.copy() or {}
            deps.update(si.TARGET_TO_PATH)
            tf.write(Path.json_dumps(deps).encode())
            tf.seek(0)

            res, run_as_root_calls = self._compile(
                [
                    *parent_feature_json,
                    "--targets-and-outputs",
                    tf.name,
                ]
            )
            self.assertEqual(
                svod.SubvolumeOnDisk(
                    **{
                        svod._BTRFS_UUID: "fake uuid",
                        svod._BTRFS_PARENT_UUID: None,
                        svod._HOSTNAME: "fake host",
                        svod._SUBVOLUMES_BASE_DIR: _SUBVOLS_DIR,
                        svod._SUBVOLUME_REL_PATH: _FAKE_SUBVOL,
                    }
                ),
                res._replace(**{svod._HOSTNAME: "fake host"}),
            )
            return run_as_root_calls

    @_subvol_mock_lexists_is_btrfs_and_run_as_root  # Mocks from _compile()
    def _expected_run_as_root_calls(
        self,
        lexists,
        is_btrfs,
        run_as_root,
        *_run_nspawns,
    ):
        "Get the commands that each of the *expected* sample items would run"
        lexists.side_effect = _os_path_lexists
        is_btrfs.return_value = True
        subvol = subvol_utils.Subvol(
            f"{_SUBVOLS_DIR}/{_FAKE_SUBVOL}", already_exists=True
        )
        layer_opts = LayerOpts(
            layer_target="fake-target",
            build_appliance=layer_resource_subvol(
                __package__, "test-build-appliance"
            ),
            artifacts_may_require_repo=True,  # Must match CLI arg in `_compile`
            target_to_path=si.TARGET_TO_PATH,
            subvolumes_dir=_SUBVOLS_DIR,
            version_set_override=None,
            rpm_installer=YumDnf.dnf,
            rpm_repo_snapshot=None,
        )
        phase_item_ids = set()
        for builder_maker, item_ids in si.ORDERED_PHASES:
            phase_item_ids.update(item_ids)
            builder_maker([si.ID_TO_ITEM[i] for i in item_ids], layer_opts)(
                subvol
            )

        for item_id, item in si.ID_TO_ITEM.items():
            if item_id not in phase_item_ids:
                item.build(subvol, layer_opts)
        return run_as_root.call_args_list + [
            (
                (
                    [
                        "btrfs",
                        "property",
                        "set",
                        "-ts",
                        f"{_SUBVOLS_DIR}/{_FAKE_SUBVOL}".encode(),
                        "ro",
                        "true",
                    ],
                ),
            ),
            ((_FIND_ARGS,), {"stdout": subprocess.PIPE}),
        ]

    def _assert_equal_call_sets(self, expected, actual):
        """
        Check that the expected & actual sets of commands are identical.
        Mock `call` objects are unhashable, so we sort.
        """

        # Compare unittest.mock call lists (which are tuple subclasses) with
        # tuples.  We need to compare `repr` because direct comparisons
        # would end up comparing `str` and `bytes` and fail.
        def tuple_repr(a):
            return repr(tuple(a))

        def fix_stdin(c):
            if isinstance(c[-1], dict):
                other = c[:-1]
                kwargs = c[-1].copy()
                if "stdin" in kwargs:
                    kwargs["stdin"] = (
                        "this makes redirected stdins comparable",
                        kwargs.pop("stdin").name,
                    )
            else:
                other = c
                kwargs = {}
            return other + (kwargs,)

        for e, a in zip(
            sorted(expected, key=tuple_repr), sorted(actual, key=tuple_repr)
        ):
            self.assertEqual(fix_stdin(e), fix_stdin(a))

    def test_compile(self):
        # First, test compilation with no parent layer.
        expected_calls = self._expected_run_as_root_calls()
        self.assertGreater(  # Sanity check: at least one command per item
            len(expected_calls), len(si.ID_TO_ITEM)
        )
        self._assert_equal_call_sets(
            expected_calls,
            self._compiler_run_as_root_calls(
                parent_feature_json=[], parent_dep={}
            ),
        )

        # Now, add an empty parent layer
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            parent = temp_subvolumes.create("parent")
            # Manually add/remove some commands from the "expected" set to
            # accommodate the fact that we have a parent subvolume.
            subvol_path = f"{_SUBVOLS_DIR}/{_FAKE_SUBVOL}".encode()
            # Our unittest.mock.call objects are (args, kwargs) pairs.
            expected_calls_with_parent = [
                c
                for c in expected_calls
                if c
                not in [
                    (
                        (["btrfs", "subvolume", "create", subvol_path],),
                        {"_subvol_exists": False},
                    ),
                    ((["chmod", "0755", subvol_path],), {}),
                    ((["chown", "root:root", subvol_path],), {}),
                ]
            ] + [
                (
                    (["test", "!", "-e", subvol_path],),
                    {"_subvol_exists": False},
                ),
                (
                    (
                        [
                            "btrfs",
                            "subvolume",
                            "snapshot",
                            parent.path(),
                            subvol_path,
                        ],
                    ),
                    {"_subvol_exists": False},
                ),
            ]
            self.assertEqual(  # We should've removed 3, and added 2 commands
                len(expected_calls_with_parent) + 1, len(expected_calls)
            )
            with mock_layer_dir_access(self, parent.path()) as parent_dir:
                with open(parent_dir / "feature.json", "w") as out_f:
                    json.dump(
                        {
                            "parent_layer": [
                                {
                                    "subvol": {
                                        "__BUCK_LAYER_TARGET": "//fake:parent"
                                    }
                                }
                            ],
                            "target": "//ignored:target",
                        },
                        out_f,
                    )
                self._assert_equal_call_sets(
                    expected_calls_with_parent,
                    self._compiler_run_as_root_calls(
                        parent_feature_json=[
                            "--child-feature-json="
                            + f'{parent_dir / "feature.json"}'
                        ],
                        parent_dep={"//fake:parent": parent_dir.decode()},
                    ),
                )


if __name__ == "__main__":
    unittest.main()
