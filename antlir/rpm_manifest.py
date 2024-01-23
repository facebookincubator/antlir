#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Given a layer, extract the installed RPMs to an output manifest file."""
import argparse
import json
import os
import re
import subprocess
import xml.etree.ElementTree as ET

from antlir.cli import normalize_buck_path

from antlir.common import init_logging
from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import generate_work_dir, Path
from antlir.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn


CVE_REGEX = re.compile(r"""\bCVE-[0-9]{4}-[0-9]+\b""")


def parse_args(argv):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--output-path",
        required=True,
        type=normalize_buck_path,
        help="Write the extracted manifest to this path",
    )
    parser.add_argument(
        "--layer",
        help="Layer to extract from",
        required=True,
    )
    parser.add_argument(
        "--build-appliance",
        help="Build appliance to use for extraction",
        required=True,
    )
    return Path.parse_args(parser, argv)


def _str_or_none(txt):
    if txt is None or txt == "(none)":
        return None
    return txt


def _int_or_none(txt):
    x = _str_or_none(txt)
    if x is None:
        return None
    return int(x)


def _xpath_to_integer(elems):
    if not elems:
        return None
    return _int_or_none(elems[0].text)


def _xpath_to_string(elems):
    if not elems:
        return None
    return _str_or_none(elems[0].text)


def _xpath_to_cves(elems):
    mset = set()
    for elem in elems:
        mset |= {m[0] for m in CVE_REGEX.finditer(elem.text)}
    return sorted(mset, reverse=True)


def _nvra_to_name(n, v, r, a):
    if a is not None:
        return "{}-{}-{}.{}".format(n, v, r, a)
    return "{}-{}-{}".format(n, v, r)


def extract_rpm_manifest(argv) -> None:
    args = parse_args(argv)
    output_path = args.output_path
    assert not os.path.exists(output_path)
    layer = find_built_subvol(args.layer)
    ba_layer = find_built_subvol(args.build_appliance)

    # C9 has moved the RPM DB
    potential_rpm_db_paths = [
        layer.path("usr/lib/sysimage/rpm"),
        layer.path("var/lib/rpm"),
    ]
    db_path_src = None
    for candidate in potential_rpm_db_paths:
        if os.path.exists(candidate):
            db_path_src = candidate
            break
    if db_path_src is None:
        raise ValueError("RPM DB not found")
    if not os.path.exists(db_path_src):
        raise ValueError(f"RPM DB paths {potential_rpm_db_paths} do not exist")
    db_path_dst = generate_work_dir()

    res, _ = run_nspawn(
        new_nspawn_opts(
            cmd=[
                "rpm",
                "--dbpath",
                db_path_dst,
                "-qa",
                "--xml",
            ],
            layer=ba_layer,
            bindmount_ro=[(db_path_src, db_path_dst)],
            # this is totally unnecessary, but this short-circuits a condition
            # here https://fburl.com/code/61e71kx7 that makes this work when run
            # in an antlir2 build appliance
            bind_repo_ro=True,
        ),
        PopenArgs(stdout=subprocess.PIPE),
    )
    root = ET.fromstring("<docroot>" + res.stdout.decode("utf-8") + "</docroot>")

    objs = []
    for hdr in root.findall("./rpmHeader"):
        n = _xpath_to_string(hdr.findall("./rpmTag[@name='Name']/string"))
        e = _xpath_to_integer(hdr.findall("./rpmTag[@name='Epoch']/integer"))
        v = _xpath_to_string(hdr.findall("./rpmTag[@name='Version']/string"))
        r = _xpath_to_string(hdr.findall("./rpmTag[@name='Release']/string"))
        a = _xpath_to_string(hdr.findall("./rpmTag[@name='Arch']/string"))
        o = _xpath_to_string(hdr.findall("./rpmTag[@name='Os']/string"))
        src = _xpath_to_string(hdr.findall("./rpmTag[@name='Sourcerpm']/string"))
        sz = _xpath_to_integer(hdr.findall("./rpmTag[@name='Size']/integer"))
        cves = _xpath_to_cves(hdr.findall("./rpmTag[@name='Changelogtext']/string"))
        name = _nvra_to_name(n, v, r, a)

        objs.append(
            {
                "name": name,
                "nevra": {
                    "name": n,
                    "epochnum": e,
                    "version": v,
                    "release": r,
                    "arch": a,
                },
                "os": o,
                "srpm": src,
                "size": sz,
                "patched_cves": cves,
            }
        )

    with open(output_path, "w") as of:
        json.dump({"rpms": objs}, of, sort_keys=True, indent=4)


def main() -> None:  # pragma: no cover
    import sys

    init_logging()
    extract_rpm_manifest(sys.argv[1:])


if __name__ == "__main__":
    main()  # pragma: no cover
