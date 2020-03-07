#!/usr/bin/env python3
import os

from fs_image.compiler.items.common import LayerOpts
from fs_image.compiler.items.install_file import InstallFileItem
from fs_image.compiler.items.make_dirs import MakeDirsItem
from fs_image.compiler.items.make_subvol import FilesystemRootItem
from fs_image.compiler.items.mount import MountItem
from fs_image.compiler.items.remove_path import RemovePathAction, RemovePathItem
from fs_image.compiler.items.rpm_action import RpmAction, RpmActionItem
from fs_image.compiler.items.symlink import SymlinkToDirItem, SymlinkToFileItem
from fs_image.compiler.items.tarball import TarballItem
from fs_image.fs_utils import Path

_NONPORTABLE_ARTIFACTS = int(os.environ.get(
    'test_image_feature_built_artifacts_require_repo'
))

T_BASE = '//fs_image/compiler/test_images'
# Use the "debug", human-readable forms of the image_feature targets here,
# since that's what we are testing.
T_DIRS = f'{T_BASE}:feature_dirs'
T_BAD_DIR = f'{T_BASE}:feature_bad_dir'
T_MOUNT = f'{T_BASE}:feature_mount'
T_SYMLINKS = f'{T_BASE}:feature_symlinks'
T_TAR = f'{T_BASE}:feature_tar_and_rpms'
T_PRINT_OK = f'{T_BASE}:print-ok'
T_EXE_WRAP_PRINT_OK = \
    f'{T_BASE}:install_buck_runnable_wrap_source__print-ok__80cbde81'
T_DIR_PRINT_OK = f'{T_BASE}:dir-print-ok'
T_DIR_WITH_SCRIPT = f'{T_BASE}:dir-with-script'
T_EXE_WRAP_DIR_PRINT_OK = \
    f'{T_BASE}:install_buck_runnable_wrap_source__dir-print-ok__2f3b9d05'
T_INSTALL_FILES = f'{T_BASE}:feature_install_files'
T_KITCHEN_SINK = f'{T_BASE}:feature_kitchen_sink'
T_HELLO_WORLD_BASE = f'{T_BASE}:hello_world_base'
T_HELLO_WORLD_TAR = f'{T_BASE}:hello_world.tar'
T_RPM_TEST_CHEESE = f'{T_BASE}:rpm-test-cheese-2-1.rpm'

TARGET_ENV_VAR_PREFIX = 'test_image_feature_path_to_'
TARGET_TO_PATH = {
    '{}:{}'.format(T_BASE, target[len(TARGET_ENV_VAR_PREFIX):]): path
        for target, path in os.environ.items()
            if target.startswith(TARGET_ENV_VAR_PREFIX)
}
# We rely on Buck setting the environment via the `env =` directive.
assert T_HELLO_WORLD_TAR in TARGET_TO_PATH, 'You must use `buck test`'


def mangle(feature_target):
    return feature_target + (
        '_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN_'
        'SO_DO_NOT_DO_THIS_EVER_PLEASE_KTHXBAI'
    )


# Shamelessly copied from `compiler/items/tests/common.py` to avoid
# dependencies.
DUMMY_LAYER_OPTS = LayerOpts(
    layer_target='fake target',  # Only used by error messages
    build_appliance=None,
    # For a handful of tests, this must be a boolean value so the layer
    # emits it it into /meta, but the value is not important.
    artifacts_may_require_repo=True,
    target_to_path=None,
    subvolumes_dir=None,
    force_yum_dnf=None,
    preserve_yum_dnf_cache=False,
    rpm_repo_snapshot='default',
    allowed_host_mount_targets=[],
)

# This should be a faithful transcription of the `image_feature`
# specifications in `test_images/TARGETS`.  The IDs currently have no
# semantics, existing only to give names to specific items.
ID_TO_ITEM = {
    '/': FilesystemRootItem(from_target=None),

    # From `feature_dirs`:
    'foo/bar': MakeDirsItem(
        from_target=T_DIRS, into_dir='/', path_to_make='/foo/bar'
    ),
    'foo/bar/baz': MakeDirsItem(
        from_target=T_DIRS, into_dir='/foo/bar', path_to_make='baz'
    ),

    # From `feature_bad_dir`:
    'foo/borf/beep': MakeDirsItem(
        from_target=T_BAD_DIR,
        into_dir='/foo',
        path_to_make='borf/beep',
        user_group='uuu:ggg',
        mode='mmm',
    ),

    # From `feature_symlinks`:
    'foo/fighter': SymlinkToDirItem(
        from_target=T_SYMLINKS,
        dest='/foo/fighter',
        source='/foo/bar',
    ),
    'foo/face': SymlinkToDirItem(
        from_target=T_SYMLINKS,
        dest='/foo/face',
        source='/foo/bar',
    ),
    'foo/bar/baz/bar': SymlinkToDirItem(  # Rsync style
        from_target=T_SYMLINKS,
        dest='/foo/bar/baz/',
        source='/foo/bar',
    ),
    'foo/hello_world.tar': InstallFileItem(
        from_target=T_SYMLINKS,
        source=Path(TARGET_TO_PATH[T_HELLO_WORLD_TAR]),
        dest='/foo/hello_world.tar',
    ),
    'foo/symlink_to_hello_world.tar': SymlinkToFileItem(
        from_target=T_SYMLINKS,
        dest='/foo/symlink_to_hello_world.tar',
        source='/foo/hello_world.tar',
    ),
    'foo/symlink_to_dev_null': SymlinkToFileItem(
        from_target=T_SYMLINKS,
        dest='/foo/symlink_to_dev_null',
        source='/dev/null',
    ),

    # From `feature_tar_and_rpms`:
    'foo/borf/hello_world': TarballItem(
        from_target=T_TAR,
        source=Path(TARGET_TO_PATH[T_HELLO_WORLD_TAR]),
        into_dir='foo/borf',
        force_root_ownership=False,
    ),
    'foo/hello_world': TarballItem(
        from_target=T_TAR,
        source=Path(TARGET_TO_PATH[T_HELLO_WORLD_TAR]),
        into_dir='foo',
        force_root_ownership=False,
    ),
    '.rpms/install/rpm-test-mice': RpmActionItem(
        from_target=T_TAR,
        name='rpm-test-mice',
        action=RpmAction.install,
    ),
    '.rpms/install/rpm-test-cheese-2-1.rpm': RpmActionItem(
        from_target=T_TAR,
        source=Path(TARGET_TO_PATH[T_RPM_TEST_CHEESE]),
        action=RpmAction.install,
    ),
    '.rpms/remove_if_exists/rpm-test-carrot': RpmActionItem(
        from_target=T_TAR,
        name='rpm-test-carrot',
        action=RpmAction.remove_if_exists,
    ),
    '.rpms/remove_if_exists/rpm-test-milk': RpmActionItem(
        from_target=T_TAR,
        name='rpm-test-milk',
        action=RpmAction.remove_if_exists,
    ),

    # From `feature_mount`:
    'meownt': MountItem(
        layer_opts=DUMMY_LAYER_OPTS,
        from_target=T_MOUNT,
        mountpoint='meownt',
        target=TARGET_TO_PATH[T_HELLO_WORLD_BASE],
        mount_config=None,
    ),
    'host_etc': MountItem(
        layer_opts=DUMMY_LAYER_OPTS,
        from_target=T_MOUNT,
        mountpoint='host_etc',
        target=None,
        mount_config={
            'is_directory': True,
            'build_source': {'type': 'host', 'source': '/etc'},
        },
    ),

    # From `feature_install_files`:
    'foo/bar/hello_world.tar': InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_HELLO_WORLD_TAR]),
        dest='/foo/bar/hello_world.tar',
    ),
    'foo/bar/hello_world_again.tar': InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_HELLO_WORLD_TAR]),
        dest='/foo/bar/hello_world_again.tar',
        user_group='nobody:nobody',
    ),
    'foo/bar/installed': MakeDirsItem(
        from_target=T_INSTALL_FILES,
        into_dir='/foo/bar',
        path_to_make='/installed',
    ),
    'foo/bar/installed/yittal-kitteh': InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_DIR_PRINT_OK]) / 'kitteh',
        dest='/foo/bar/installed/yittal-kitteh',
    ),
    'foo/bar/installed/print-ok': InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[
            T_EXE_WRAP_PRINT_OK if _NONPORTABLE_ARTIFACTS else T_PRINT_OK
        ]),
        dest='/foo/bar/installed/print-ok',
    ),
    'foo/bar/installed/print-ok-too': InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_EXE_WRAP_DIR_PRINT_OK])
            if _NONPORTABLE_ARTIFACTS else (
                Path(TARGET_TO_PATH[T_DIR_PRINT_OK]) / 'subdir/print-ok'
            ),
        dest='/foo/bar/installed/print-ok-too',
    ),
    'foo/bar/installed/script-dir': InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_DIR_WITH_SCRIPT]),
        dest='/foo/bar/installed/script-dir',
    ),
    'foo/bar/installed/solo-exe.sh': InstallFileItem(
        from_target=T_INSTALL_FILES,
        source=Path(TARGET_TO_PATH[T_DIR_WITH_SCRIPT]) / 'subdir/exe.sh',
        dest='/foo/bar/installed/solo-exe.sh',
    ),

    # From `feature_kitchen_sink`:
    '.remove_if_exists/path/to/remove': RemovePathItem(
        from_target=T_KITCHEN_SINK,
        path='/path/to/remove',
        action=RemovePathAction.if_exists,
    ),
    '.remove_assert_exists/path/to/remove': RemovePathItem(
        from_target=T_KITCHEN_SINK,
        path='/path/to/remove',
        action=RemovePathAction.assert_exists,
    ),
    '.remove_assert_exists/another/path/to/remove': RemovePathItem(
        from_target=T_KITCHEN_SINK,
        path='/another/path/to/remove',
        action=RemovePathAction.assert_exists,
    ),
}


# Imitates the output of `DependencyGraph.ordered_phases` for `test-compiler`
ORDERED_PHASES = (
    (FilesystemRootItem.get_phase_builder, ['/']),
    (RpmActionItem.get_phase_builder, [
        '.rpms/install/rpm-test-mice',
        '.rpms/install/rpm-test-cheese-2-1.rpm'
    ]),
    (RpmActionItem.get_phase_builder, [
        '.rpms/remove_if_exists/rpm-test-carrot',
        '.rpms/remove_if_exists/rpm-test-milk',
    ]),
    (RemovePathItem.get_phase_builder, [
        '.remove_if_exists/path/to/remove',
        '.remove_assert_exists/path/to/remove',
        '.remove_assert_exists/another/path/to/remove',
    ]),
)
