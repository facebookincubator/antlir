#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# pyre-strict

import time
from pathlib import Path

import click
import createrepo_c as cr


_COMPRESSION_MODES = {
    "none": ("", cr.NO_COMPRESSION),
    "gzip": (".gz", cr.GZ_COMPRESSION),
}


@click.command()
@click.option(
    "--primary-dir",
    type=click.Path(exists=True, file_okay=False, dir_okay=True, path_type=Path),
    required=True,
)
@click.option(
    "--filelists-dir",
    type=click.Path(exists=True, file_okay=False, dir_okay=True, path_type=Path),
    required=True,
)
@click.option(
    "--other-dir",
    type=click.Path(exists=True, file_okay=False, dir_okay=True, path_type=Path),
    required=True,
)
@click.option(
    "--out",
    type=click.Path(exists=False, file_okay=False, dir_okay=True, path_type=Path),
    required=True,
)
@click.option(
    "--timestamp",
    type=int,
    default=int(time.time()),
)
@click.option(
    "--compress",
    type=click.Choice(_COMPRESSION_MODES.keys()),
    default="gzip",
)
def main(
    primary_dir: Path,
    filelists_dir: Path,
    other_dir: Path,
    out: Path,
    timestamp: int,
    compress: str,
) -> int:
    out.mkdir()
    ext = _COMPRESSION_MODES[compress][0]
    paths = {
        "primary": out / f"primary.xml{ext}",
        "filelists": out / f"filelists.xml{ext}",
        "other": out / f"other.xml{ext}",
    }
    compress = _COMPRESSION_MODES[compress][1]
    files = {
        "primary": cr.PrimaryXmlFile(str(paths["primary"]), compress),
        "filelists": cr.FilelistsXmlFile(str(paths["filelists"]), compress),
        "other": cr.OtherXmlFile(str(paths["other"]), compress),
    }
    inputdirs = {
        "primary": primary_dir,
        "filelists": filelists_dir,
        "other": other_dir,
    }
    for (name, inputdir) in inputdirs.items():
        xml_paths = sorted(inputdir.iterdir())
        files[name].set_num_of_pkgs(len(xml_paths))
        for path in xml_paths:
            with open(path) as f:
                files[name].add_chunk(f.read())

    for file in files.values():
        file.close()

    repomd = cr.Repomd()
    for (name, path) in paths.items():
        record = cr.RepomdRecord(name, str(path))
        record.set_timestamp(timestamp)
        record.fill(cr.SHA256)
        repomd.set_record(record)
    repomd.set_revision(str(timestamp))
    with open(out / "repomd.xml", "w") as f:
        f.write(repomd.xml_dump())
    return 0


if __name__ == "__main__":
    main()
