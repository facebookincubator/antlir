# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

#!/usr/bin/env python3

# Generate a set of TARGETS files for upstream repos. This *DOES NOT* do
# anything to ensure that the contents are preserved, so should only be used
# with either:
#  * long-lived upstream repos (where rpms don't get deleted often / at all)
#  * frequent generated source updates (so referenced links don't disappear)

import argparse
import pprint
import tempfile
from contextlib import ExitStack
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional
from urllib.parse import ParseResult, urljoin, urlparse, urlunparse

import createrepo_c as cr
import requests


@dataclass
class rpm(object):
    name: str
    rpm_name: str
    epoch: int
    version: str
    release: str
    arch: str
    url: str
    xml: str
    sha1: Optional[str] = None
    sha256: Optional[str] = None


@dataclass
class xml(object):
    name: str
    primary: str
    filelists: str
    other: str


@dataclass
class repo(object):
    name: str
    rpms: List[str]
    visibility: List[str]


@dataclass
class repo_set(object):
    name: str
    repos: List[str]
    visibility: List[str]


def snapshot_repo(args, base_url: ParseResult) -> str:
    repo_id = base_url.path.strip("/").replace("/", "_")
    base_url = urlunparse(base_url)
    repomd = requests.get(urljoin(base_url, "repodata/repomd.xml"))
    with tempfile.NamedTemporaryFile("wb") as f:
        f.write(repomd.content)
        f.flush()
        repomd = cr.Repomd(f.name)
    repomd = {
        r.type: r for r in repomd.records if r.type in {"primary", "filelists", "other"}
    }
    xmls = {
        r.type: requests.get(urljoin(base_url, r.location_href)).content
        for r in repomd.values()
    }

    targets = []
    rpm_target_names = []

    with ExitStack() as stack:
        xml_files = {
            typ: stack.enter_context(tempfile.NamedTemporaryFile("wb")) for typ in xmls
        }
        for typ, body in xmls.items():
            xml_files[typ].write(body)
            xml_files[typ].flush()

        for pkg in cr.PackageIterator(
            xml_files["primary"].name,
            xml_files["filelists"].name,
            xml_files["other"].name,
        ):
            target_name = pkg.nevra().replace("^", "").replace(":", "/")
            targets.append(
                xml(
                    name=target_name + "--xml",
                    primary=cr.xml_dump_primary(pkg),
                    filelists=cr.xml_dump_filelists(pkg),
                    other=cr.xml_dump_other(pkg),
                )
            )
            targets.append(
                rpm(
                    name=target_name,
                    rpm_name=pkg.name,
                    epoch=int(pkg.epoch),
                    version=pkg.version,
                    release=pkg.release,
                    arch=pkg.arch,
                    url=urljoin(base_url, pkg.location_href),
                    xml=":" + target_name + "--xml",
                    **{pkg.checksum_type: pkg.pkgId}
                )
            )
            rpm_target_names.append(":" + target_name)

    targets = sorted(repr(t) for t in targets)
    source = """
load("//antlir/antlir2/package_managers/dnf/rules:repo.bzl", "repo")
load("//antlir/antlir2/package_managers/dnf/rules:rpm.bzl", "rpm")
load("//antlir/antlir2/package_managers/dnf/rules:xml.bzl", "xml")

"""
    source += (
        pprint.pformat(repo(name=repo_id, rpms=rpm_target_names, visibility=["PUBLIC"]))
        + "\n\n\n"
    )
    source += "\n\n".join(targets)

    return source


def main(args) -> None:
    args.dst.mkdir(parents=True, exist_ok=True)
    repos = {}
    for base_url in args.baseurls:
        source = snapshot_repo(args, base_url)
        buck_file = args.dst / base_url.path.lstrip("/") / "BUCK"
        buck_file.parent.mkdir(parents=True, exist_ok=True)
        with open(buck_file, "w") as f:
            f.write(source)

        repo_id = base_url.path.strip("/").replace("/", "_")
        repos[base_url.path.strip("/")] = repo_id
    with open(args.dst / "BUCK", "w") as f:
        f.write(
            'load("//antlir/antlir2/package_managers/dnf/rules:repo.bzl", "repo_set")\n\n'
        )
        repo_targets = [
            "//" + str(args.dst) + "/" + path + ":" + repo_id
            for path, repo_id in repos.items()
        ]
        f.write(
            pprint.pformat(
                repo_set(name="repos", repos=repo_targets, visibility=["PUBLIC"])
            )
        )
        f.write("\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--dst", type=Path)
    parser.add_argument("baseurls", nargs="+", type=urlparse)
    main(parser.parse_args())
