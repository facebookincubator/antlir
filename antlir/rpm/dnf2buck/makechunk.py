#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# pyre-strict

import enum
import json
from pathlib import Path

import click
import createrepo_c as cr


class ChunkType(enum.Enum):
    PRIMARY = "primary"
    FILELISTS = "filelists"
    OTHER = "other"


@click.command()
@click.option(
    "--rpm", type=click.Path(exists=True, dir_okay=False, path_type=Path), required=True
)
@click.option("--out", type=click.File("w"), required=True)
@click.option("--href", required=True)
# pyre-fixme[2]: Parameter must be annotated.
def main(rpm: Path, out, href: str) -> int:
    pkg = cr.package_from_rpm(str(rpm))
    # We would never care about the time it was materialized on disk, just when
    # it was built. The sha256 is encoded at build time, so the package can't
    # change anyway.
    pkg.time_file = pkg.time_build
    pkg.location_href = href

    json.dump(
        {
            "primary": cr.xml_dump_primary(pkg),
            "filelists": cr.xml_dump_filelists(pkg),
            "other": cr.xml_dump_other(pkg),
        },
        out,
    )
    return 0


if __name__ == "__main__":
    main()
