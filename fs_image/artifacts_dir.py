#!/usr/bin/env python3
'DANGER: The resulting PAR will not work if copied outside of buck-out.'
import os
import shutil
import subprocess


def _maybe_make_symlink_to_scratch(
    symlink_path, target_in_scratch, path_in_repo,
):
    '''
    IMPORTANT: This must be safe against races with other concurrent copies
    of `artifacts_dir.py`.
    '''
    scratch_bin = shutil.which('mkscratch')
    if scratch_bin is None:
        return symlink_path

    # If the decode ever fails, we should probably switch this entire
    # function to return `bytes`.
    target_path, _ = subprocess.check_output([
        scratch_bin, 'path', '--subdir', target_in_scratch, path_in_repo,
    ]).decode().rsplit('\n', 1)

    # Atomically ensure the desired symlink exists.
    try:
        os.symlink(target_path, symlink_path)
    except FileExistsError:
        pass

    # These two error conditions should never happen under normal usage, so
    # they are left as exceptions instead of auto-remediations.
    if not os.path.islink(symlink_path):
        raise RuntimeError(
            f'{symlink_path} is not a symlink. Clean up whatever is there '
            'and try again?'
        )
    real_target = os.path.realpath(symlink_path)
    if real_target != target_path:
        raise RuntimeError(
            f'{symlink_path} points at {real_target}, but should point '
            f'at {target_path}. Clean this up, and try again?'
        )

    return target_path


def find_repo_root(path_in_repo: str) -> str:
    '''
    The caller is responsible for providing a path known to be in the repo.
    We cannot just use __file__, because that will never in the repo in
    tests running under @mode/opt. For that reason, tests should just pass
    sys.argv[0].

    This is intended to work:
     - under Buck's internal macro interpreter, and
     - using the system python from `facebookexperimental/buckit`.
    '''
    repo_path = os.path.abspath(path_in_repo)
    while True:
        repo_path = os.path.dirname(repo_path)
        if repo_path == '/':
            raise RuntimeError(
                'Could not find .buckconfig in any ancestor of '
                f'{os.path.dirname(os.path.realpath(__file__))}'
            )
        if os.path.exists(os.path.join(repo_path, '.buckconfig')):
            return repo_path
    # Not reached


def ensure_per_repo_artifacts_dir_exists(path_in_repo: str) -> str:
    "See `find_repo_root`'s docblock to understand `path_in_repo`"
    repo_path = find_repo_root(path_in_repo)
    artifacts_dir = os.path.join(repo_path, 'buck-image-out')

    # On Facebook infra, the repo might be hosted on an Eden filesystem,
    # which is not intended as a backing store for a large sparse loop
    # device filesystem.  So, we will put our artifacts in a blessed scratch
    # space instead.
    #
    # The location in the scratch directory is a hardcoded path because
    # this really must be a per-repo singleton.
    real_dir = _maybe_make_symlink_to_scratch(
        artifacts_dir, 'buck-image-out', repo_path
    )

    try:
        os.mkdir(real_dir)
    except FileExistsError:
        pass  # May race with another mkdir from a concurrent artifacts_dir.py
    return artifacts_dir


if __name__ == '__main__':
    import sys
    print(ensure_per_repo_artifacts_dir_exists(sys.argv[0]))
