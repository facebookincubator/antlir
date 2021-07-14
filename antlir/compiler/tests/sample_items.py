#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os

from antlir.compiler.items.common import LayerOpts
from antlir.compiler.items.ensure_dirs_exist import EnsureDirsExistItem
from antlir.compiler.items.group import GroupItem
from antlir.compiler.items.install_file import InstallFileItem
from antlir.compiler.items.make_subvol import FilesystemRootItem
from antlir.compiler.items.mount import MountItem
from antlir.compiler.items.remove_path import RemovePathItem
from antlir.compiler.items.rpm_action import RpmAction, RpmActionItem
from antlir.compiler.items.symlink import SymlinkToDirItem, SymlinkToFileItem
from antlir.compiler.items.tarball import TarballItem
from antlir.compiler.items.user import UserItem
from antlir.fs_utils import Path


_NONPORTABLE_ARTIFACTS = int(
    # pyre-fixme[6]: Expected `Union[_SupportsTrunc, bytes, str,
    #  typing.SupportsInt, typing_extensions.SupportsIndex]` for 1st param but
    #  got `Optional[str]`.
    os.environ.get("test_image_feature_built_artifacts_require_repo")
)

T_BASE = "//antlir/compiler/test_images"
# Use the "debug", human-readable forms of the `feature`s targets here,
# since that's what we are testing.
T_DIRS = f"{T_BASE}:feature_dirs"
T_BAD_DIR = f"{T_BASE}:feature_bad_dir"
T_BAD_DIR_MODE_MISMATCH = f"{T_BASE}:feature_bad_dir_mode_mismatch"
T_MOUNT = f"{T_BASE}:feature_mount"
T_SYMLINKS = f"{T_BASE}:feature_symlinks"
T_TAR = f"{T_BASE}:feature_tar_and_rpms"
T_PRINT_OK = f"{T_BASE}:print-ok"
T_EXE_WRAP_PRINT_OK = (
    f"{T_BASE}:"
    + "print-ok__"
    + "install_buck_runnable_wrap_source-print-ok__tZD0yB-oR8iaa7Q8gX1u"
)
T_DIR_PRINT_OK = f"{T_BASE}:dir-print-ok"
T_DIR_WITH_SCRIPT = f"{T_BASE}:dir-with-script"
T_EXE_WRAP_DIR_PRINT_OK = (
    f"{T_BASE}:"
    + "dir-print-ok__"
    + "install_buck_runnable_wrap_source-dir-print-ok__aSw-wLLqmDSnGfAZQzPs"
)
T_INSTALL_FILES = f"{T_BASE}:feature_install_files"
T_KITCHEN_SINK = f"{T_BASE}:feature_kitchen_sink"
T_HELLO_WORLD_BASE = f"{T_BASE}:hello_world_base"
T_HELLO_WORLD_TAR = f"{T_BASE}:hello_world.tar"
T_RPM_TEST_CHEESE = f"{T_BASE}:rpm-test-cheese-2-1.rpm"
T_SHADOW_ME = f"{T_BASE}:shadow_me"

TARGET_ENV_VAR_PREFIX = "test_image_feature_path_to_"
TARGET_TO_PATH = {
    "{}:{}".format(T_BASE, target[len(TARGET_ENV_VAR_PREFIX) :]): path
    for target, path in os.environ.items()
    if target.startswith(TARGET_ENV_VAR_PREFIX)
}
# We rely on Buck setting the environment via the `env =` directive.
assert T_HELLO_WORLD_TAR in TARGET_TO_PATH, "You must use `buck test`"


def mangle(feature_target):
    return feature_target + (
        "_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN"
    )


# Shamelessly copied from `compiler/items/tests/common.py` to avoid
# dependencies.
DUMMY_LAYER_OPTS = LayerOpts(
    layer_target="fake target",  # Only used by error messages
    build_appliance=None,
    # For a handful of tests, this must be a boolean value so the layer
    # emits it it into /.meta, but the value is not important.
    artifacts_may_require_repo=True,
    # pyre-fixme[6]: Expected `Mapping[str, str]` for 4th param but got `None`.
    target_to_path=None,
    subvolumes_dir=None,
    version_set_override=None,
    rpm_installer=None,
    rpm_repo_snapshot=None,
    allowed_host_mount_targets=[],
    flavor="flavor",
)

# This should be a faithful transcription of the `feature`
# specifications in `test_images/TARGETS`.  The IDs currently have no
# semantics, existing only to give names to specific items.
ID_TO_ITEM = {
    "/": FilesystemRootItem(from_target=None),
    # From `feature_dirs`:
    "foo": EnsureDirsExistItem(
        from_target=T_DIRS, into_dir="/", basename="foo"
    ),
    "foo/bar": EnsureDirsExistItem(
        from_target=T_DIRS, into_dir="/foo", basename="bar"
    ),
    "foo/bar/baz": EnsureDirsExistItem(
        from_target=T_DIRS, into_dir="/foo/bar", basename="baz"
    ),
    "alpha": EnsureDirsExistItem(
        from_target=T_DIRS, into_dir="/", basename="alpha", mode="a+rx"
    ),
    "alpha/beta": EnsureDirsExistItem(
        from_target=T_DIRS, into_dir="/alpha", basename="beta", mode="a+rwx"
    ),
    # From `feature_bad_dir_mode_mismatch`:
    "bad_mode:alpha": EnsureDirsExistItem(
        from_target=T_BAD_DIR_MODE_MISMATCH,
        into_dir="/",
        basename="alpha",
        mode="a+rwx",
    ),
    # From `feature_bad_dir`:
    "foo/borf": EnsureDirsExistItem(
        from_target=T_BAD_DIR,
        into_dir="/foo",
        basename="borf",
        user_group="uuu:ggg",
        mode="mmm",
    ),
    "foo/borf/beep": EnsureDirsExistItem(
        from_target=T_BAD_DIR,
        into_dir="/foo/borf",
        basename="beep",
        user_group="uuu:ggg",
        mode="mmm",
    ),
    # From `feature_symlinks`:
    "foo/fighter": SymlinkToDirItem(
        from_target=T_SYMLINKS, dest="/foo/fighter", source="/foo/bar"
    ),
    "foo/face": SymlinkToDirItem(
        from_target=T_SYMLINKS, dest="/foo/face", source="/foo/bar"
    ),
    "foo/bar/baz/bar": SymlinkToDirItem(  # Rsync style
        from_target=T_SYMLINKS, dest="/foo/bar/baz/", source="/foo/bar"
    ),
    "foo/hello_world.tar": InstallFileItem(
        from_target=T_SYMLINKS,
        source=Path(TARGET_TO_PATH[T_HELLO_WORLD_TAR]),
        dest="/foo/hello_world.tar",
    ),
    "foo/symlink_to_hello_world.tar": SymlinkToFileItem(
        from_target=T_SYMLINKS,
        dest="/foo/symlink_to_hello_world.tar",
        source="/foo/hello_world.tar",
    ),
    "foo/symlink_to_dev_null": SymlinkToFileItem(
        from_target=T_SYMLINKS,
        dest="/foo/symlink_to_dev_null",
        source="/dev/null",
    ),
    # From `feature_tar_and_rpms`:
    "foo/borf/hello_world": TarballItem(
        from_target=T_TAR,
        source=Path(TARGET_TO_PATH[T_HELLO_WORLD_TAR]),
        into_dir="foo/borf",
        force_root_ownership=False,
    ),
    "foo/hello_world": TarballItem(
        from_target=T_TAR,
        source=Path(TARGET_TO_PATH[T_HELLO_WORLD_TAR]),
        into_dir="foo",
        force_root_ownership=False,
    ),
    ".rpms/install/rpm-test-mice": RpmActionItem(
        from_target=T_TAR, name="rpm-test-mice", action=RpmAction.install
    ),
    ".rpms/install/rpm-test-cheese-2-1.rpm": RpmActionItem(
        from_target=T_TAR,
        source=Path(TARGET_TO_PATH[T_RPM_TEST_CHEESE]),
        action=RpmAction.install,
    ),
    ".rpms/remove_if_exists/rpm-test-carrot": RpmActionItem(
        from_target=T_TAR,
        name="rpm-test-carrot",
        action=RpmAction.remove_if_exists,
    ),
    ".rpms/remove_if_exists/rpm-test-milk": RpmActionItem(
        from_target=T_TAR,
        name="rpm-test-milk",
        action=RpmAction.remove_if_exists,
    ),
    # From `feature_mount`:
    "meownt": MountItem(
        layer_opts=DUMMY_LAYER_OPTS,
        from_target=T_MOUNT,
        mountpoint="meownt",
        target=TARGET_TO_PATH[T_HELLO_WORLD_BASE],
        mount_config=None,
    ),
    "host_etc": MountItem(
        layer_opts=DUMMY_LAYER_OPTS,
        from_target=T_MOUNT,
        mountpoint="host_etc",
        target=None,
        mount_config={
            "is_directory": True,
            "build_source": {"type": "host", "source": "/etc"},
        },
    ),
    "dev_null": MountItem(
        layer_opts=DUMMY_LAYER_OPTS,
        from_target=T_MOUNT,
        mountpoint="dev_null",
        target=None,
        mount_config={
            "is_directory": False,
            "build_source": {"type": "host", "source": "/dev/null"},
        },
    ),
    # From `feature_install_files`:
    "foo/bar/hello_world.tar": InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_HELLO_WORLD_TAR]),
        dest="/foo/bar/hello_world.tar",
    ),
    "foo/bar/hello_world_again.tar": InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_HELLO_WORLD_TAR]),
        dest="/foo/bar/hello_world_again.tar",
        user_group="root:root",
    ),
    "foo/bar/installed": EnsureDirsExistItem(
        from_target=T_INSTALL_FILES,
        into_dir="/foo/bar",
        basename="/installed",
    ),
    "foo/bar/installed/yittal-kitteh": InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_DIR_PRINT_OK]) / "kitteh",
        dest="/foo/bar/installed/yittal-kitteh",
    ),
    "foo/bar/installed/print-ok": InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(
            TARGET_TO_PATH[
                T_EXE_WRAP_PRINT_OK if _NONPORTABLE_ARTIFACTS else T_PRINT_OK
            ]
        ),
        dest="/foo/bar/installed/print-ok",
    ),
    "foo/bar/installed/print-ok-too": InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_EXE_WRAP_DIR_PRINT_OK])
        if _NONPORTABLE_ARTIFACTS
        else (Path(TARGET_TO_PATH[T_DIR_PRINT_OK]) / "subdir/print-ok"),
        dest="/foo/bar/installed/print-ok-too",
    ),
    "foo/bar/installed/script-dir": InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_DIR_WITH_SCRIPT]),
        dest="/foo/bar/installed/script-dir",
    ),
    "foo/bar/installed/solo-exe.sh": InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_DIR_WITH_SCRIPT]) / "subdir/exe.sh",
        dest="/foo/bar/installed/solo-exe.sh",
    ),
    # From `feature_kitchen_sink`:
    ".remove_if_exists/path/to/remove": RemovePathItem(
        from_target=T_KITCHEN_SINK,
        path="/path/to/remove",
        must_exist=False,
    ),
    ".remove_assert_exists/path/to/remove": RemovePathItem(
        from_target=T_KITCHEN_SINK,
        path="/path/to/remove",
        must_exist=True,
    ),
    ".remove_assert_exists/another/path/to/remove": RemovePathItem(
        from_target=T_KITCHEN_SINK,
        path="/another/path/to/remove",
        must_exist=True,
    ),
    "etc": EnsureDirsExistItem(
        from_target=T_BAD_DIR, into_dir="/", basename="etc"
    ),
    "etc/passwd": InstallFileItem(
        from_target=T_BAD_DIR,
        source=Path(TARGET_TO_PATH[T_SHADOW_ME]),
        dest="/etc/passwd",
    ),
    "etc/group": InstallFileItem(
        from_target=T_BAD_DIR,
        source=Path(TARGET_TO_PATH[T_SHADOW_ME]),
        dest="/etc/group",
    ),
    ".group/ggg": GroupItem(
        from_target=T_BAD_DIR,
        name="ggg",
    ),
    ".user/uuu": UserItem(
        from_target=T_BAD_DIR,
        name="uuu",
        primary_group="ggg",
        supplementary_groups=[],
        shell="/foo/bar/installed/print-ok",
        home_dir="/foo/bar",
    ),
}


# Imitates the output of `DependencyGraph.ordered_phases` for `test-compiler`
ORDERED_PHASES = (
    (FilesystemRootItem.get_phase_builder, ["/"]),
    (
        RpmActionItem.get_phase_builder,
        [
            ".rpms/install/rpm-test-mice",
            ".rpms/install/rpm-test-cheese-2-1.rpm",
        ],
    ),
    (
        RpmActionItem.get_phase_builder,
        [
            ".rpms/remove_if_exists/rpm-test-carrot",
            ".rpms/remove_if_exists/rpm-test-milk",
        ],
    ),
    (
        RemovePathItem.get_phase_builder,
        [
            ".remove_if_exists/path/to/remove",
            ".remove_assert_exists/path/to/remove",
            ".remove_assert_exists/another/path/to/remove",
        ],
    ),
)
