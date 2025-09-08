#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Generate a set of TARGETS files for upstream repos. This *DOES NOT* do
# anything to ensure that the contents are preserved, so should only be used
# with either:
#  * long-lived upstream repos (where rpms don't get deleted often / at all)
#  * frequent generated source updates (so referenced links don't disappear)

import argparse
import pprint
import shutil
import sys
import tempfile
from contextlib import ExitStack
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import ParseResult, urljoin, urlparse, urlunparse

import createrepo_c as cr
import requests


@dataclass
class rpm:
    name: str
    rpm_name: str
    epoch: int
    version: str
    release: str
    arch: str
    url: str
    xml: str
    sha1: str | None = None
    sha256: str | None = None
    visibility: list[str] | None = None

    @property
    def pkgid(self) -> str:
        checksum = self.sha256 or self.sha1
        assert checksum is not None
        return checksum


@dataclass
class xml:
    name: str
    primary: str
    filelists: str
    other: str


@dataclass
class repo:
    name: str
    rpms: list[str]
    visibility: list[str]


@dataclass
class repo_set:
    name: str
    repos: list[str]
    visibility: list[str]


@dataclass
class SnapshottedRepo:
    repo: repo
    rpms: dict[str, list[tuple[rpm, xml]]]


def snapshot_repo(args, base_url: ParseResult) -> SnapshottedRepo:
    repo_id = base_url.path.strip("/").replace("/", "_")
    base_url = urlunparse(base_url)
    repomd = requests.get(urljoin(base_url, "repodata/repomd.xml"))
    with tempfile.NamedTemporaryFile("wb", suffix=".xml") as f:
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

    with ExitStack() as stack:
        xml_files = {
            typ: stack.enter_context(tempfile.NamedTemporaryFile("wb")) for typ in xmls
        }
        for typ, body in xmls.items():
            xml_files[typ].write(body)
            xml_files[typ].flush()

        rpm_targets_files = {}
        rpm_targets = []

        for pkg in cr.PackageIterator(
            xml_files["primary"].name,
            xml_files["filelists"].name,
            xml_files["other"].name,
        ):
            target_name = f"{pkg.epoch}-{pkg.version}-{pkg.release}.{pkg.arch}-{pkg.pkgId[:5]}".replace(
                "^", "_"
            ).replace(":", "_")
            url = urljoin(base_url, pkg.location_href)
            pkg.location_href = str(
                Path("Packages") / pkg.pkgId / (pkg.nevra() + ".rpm")
            )
            rpm_targets_files.setdefault(pkg.name, [])
            rpm_targets_files[pkg.name].append(
                (
                    rpm(
                        name=target_name,
                        rpm_name=pkg.name,
                        epoch=int(pkg.epoch),
                        version=pkg.version,
                        release=pkg.release,
                        arch=pkg.arch,
                        url=url,
                        xml=":" + target_name + "--xml",
                        visibility=["//" + str(args.dst) + "/..."],
                        **{pkg.checksum_type: pkg.pkgId},
                    ),
                    xml(
                        name=target_name + "--xml",
                        primary=cr.xml_dump_primary(pkg),
                        filelists=cr.xml_dump_filelists(pkg),
                        other=cr.xml_dump_other(pkg),
                    ),
                )
            )
            rpm_targets.append(
                "//" + str(args.dst) + "/rpms/" + pkg.name + ":" + target_name
            )

    return SnapshottedRepo(
        repo=repo(name=repo_id, rpms=rpm_targets, visibility=["PUBLIC"]),
        rpms=rpm_targets_files,
    )


def main(args) -> None:
    args.dst.mkdir(parents=True, exist_ok=True)
    shutil.rmtree(args.dst)
    repos = {}
    rpms = {}
    for base_url in args.baseurls:
        try:
            snap = snapshot_repo(args, base_url)
        except Exception:
            print(f"Failed to snapshot {base_url}", file=sys.stderr)
            raise
        repo_dir = args.dst / base_url.path.lstrip("/")
        buck_file = repo_dir / "BUCK"
        buck_file.parent.mkdir(parents=True, exist_ok=True)
        with open(buck_file, "w") as f:
            f.write(
                """# \x40generated
load("@antlir//antlir/antlir2/package_managers/dnf/rules:repo.bzl", "repo")
"""
            )
            f.write(repr(snap.repo))
            f.write("\n")
        for rpm_name, this in snap.rpms.items():
            rpms.setdefault(rpm_name, [])
            rpms[rpm_name].extend(this)

        repo_id = base_url.path.strip("/").replace("/", "_")
        repos[base_url.path.strip("/")] = repo_id

    for rpm_name, versions in rpms.items():
        rpm_dir = args.dst / "rpms" / rpm_name
        rpm_dir.mkdir(parents=True, exist_ok=True)
        with open(rpm_dir / "BUCK", "w") as f:
            f.write(
                """# \x40generated
load("@antlir//antlir/antlir2/package_managers/dnf/rules:rpm.bzl", "rpm")
load("@antlir//antlir/antlir2/package_managers/dnf/rules:xml.bzl", "xml")
"""
            )
            versions = {rpm.pkgid: (rpm, xml) for (rpm, xml) in versions}
            for rpm, xml in versions.values():
                f.write(repr(rpm))
                f.write("\n")
                f.write(repr(xml))
                f.write("\n")

    with open(args.dst / "BUCK", "w") as f:
        f.write("# \x40generated\n")
        f.write(
            'load("@antlir//antlir/antlir2/package_managers/dnf/rules:repo.bzl", "repo_set")\n\n'
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


def invoke_main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dst", type=Path)
    parser.add_argument("baseurls", nargs="+", type=urlparse)
    main(parser.parse_args())


if __name__ == "__main__":
    invoke_main()  # pragma: no cover
