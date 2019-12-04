#!/usr/bin/env python3
'''
Runs `yum` against an ephemeral snapshot of the `temp_repos.py` test repos.
Used in the image compiler & build appliance unit tests.
'''
import json
import os

from typing import AnyStr, List

from ..common import init_logging, Path
from ..yum_from_snapshot import add_common_yum_args, yum_from_snapshot
from fs_image.common import load_location


def yum_from_test_snapshot(
    install_root: AnyStr,
    protected_paths: List[AnyStr],
    yum_args: List[AnyStr],
):
    snapshot_dir = Path(load_location('rpm', 'repo-snapshot'))
    yum_from_snapshot(
        storage_cfg=json.dumps({
            'key': 'test',
            'kind': 'filesystem',
            'base_dir': (snapshot_dir / 'storage').decode(),
        }),
        snapshot_dir=snapshot_dir / 'repos',
        install_root=Path(install_root),
        protected_paths=protected_paths,
        yum_args=yum_args,
    )


# CLI tested indirectly via the image compiler's test image targets.
if __name__ == '__main__':  # pragma: no cover
    import argparse

    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    add_common_yum_args(parser)
    args = parser.parse_args()

    init_logging()

    yum_from_test_snapshot(
        args.install_root, args.protected_path, args.yum_args,
    )
