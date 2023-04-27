#!/usr/bin/python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Use the DNF api to pre-build .solv{x} files so that subsequent DNF runs don't
# need to re-parse and process the (giant) xml blobs every single time the repo
# is being used
# NOTE: this must be run with system python, so cannot be a PAR file

import shutil
import sys
from pathlib import Path
from tempfile import TemporaryDirectory

import dnf


def main(id: str, repodata: Path, repodata_out: Path):
    with TemporaryDirectory() as tmpdir:
        with dnf.Base() as base:
            base.conf.cachedir = tmpdir
            base.repos.add_new_repo(id, base.conf, [str(repodata.parent)])
            base.fill_sack(load_system_repo=False)
        tmpdir = Path(tmpdir)
        shutil.copytree(repodata, repodata_out)
        shutil.copyfile(tmpdir / (id + ".solv"), repodata_out / (id + ".solv"))
        shutil.copyfile(
            tmpdir / (id + "-filenames.solvx"), repodata_out / (id + "-filenames.solvx")
        )


if __name__ == "__main__":
    main(sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]))
