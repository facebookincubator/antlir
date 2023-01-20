#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
The Buck macro `rpm_repo_snapshot` takes a bare RPM repo snapshot, and
builds it up to contain configuration and binaries necessary to install
RPMs from that snapshot inside a container.

This binary here is a helper that consumes the snapshot's `{yum,dnf}.conf`
files, as they were captured from the live repo (caveat: `snapshot_repos.py`
does some light pre-mangling), rewrites them to be ready for use with our
`repo-server`, and outputs the new configs (together with supporting files)
into a directory.
"""
import argparse
import os
import textwrap
from typing import Iterable, List, TextIO
from urllib.parse import urlparse, urlunparse

from antlir.fs_utils import create_ro, Path, populate_temp_dir_and_rename
from antlir.rpm.yum_dnf_conf import YumDnf, YumDnfConfParser


def populate_versionlock_conf(
    yum_dnf: YumDnf, out_dir: Path, install_dir: Path
) -> None:
    with create_ro(out_dir / "versionlock.conf", "w") as outf:
        outf.write(
            textwrap.dedent(
                f"""\
            [main]
            enabled = 1
            locklist = {install_dir.decode()}/versionlock.list
        """
            )
        )

    # Write an empty lock-list. This will be bind-mounted in at runtime.
    with create_ro(out_dir / "versionlock.list", "w"):
        pass


def write_yum_dnf_conf(
    *,
    yum_dnf: YumDnf,
    infile: TextIO,
    out_dir: Path,
    install_dir: Path,
    ports: Iterable[int],
) -> None:
    # `yum-dnf-from-snapshot` implicitly depends on this path convention for
    # the main config and for the plugin configs under `<snapshot_dir>/etc`.
    plugin_dir = f"etc/{yum_dnf.value}/plugins"
    cache_dir = f"var/cache/{yum_dnf.value}"  # created via genrule_layer later
    config_path = f"etc/{yum_dnf.value}/{yum_dnf.value}.conf"

    os.makedirs(out_dir / plugin_dir)
    populate_versionlock_conf(
        yum_dnf,
        out_dir=out_dir / plugin_dir,
        install_dir=install_dir / plugin_dir,
    )

    server_urls = [urlparse(f"http://localhost:{p}") for p in ports]
    yc = YumDnfConfParser(yum_dnf, infile)
    isolated_yc = (
        yc.isolate()
        .isolate_repos(
            repo._replace(
                base_url=[
                    urlunparse(url._replace(path=repo.name)) for url in server_urls
                ],
                gpg_key_urls=[
                    urlunparse(
                        # NB: It's be "better" to use `random.choice` but it
                        # makes it harder to write tests, so worse it is.
                        server_urls[0]._replace(
                            path=os.path.join(
                                repo.name,
                                os.path.basename(urlparse(key_url).path),
                            )
                        )
                    )
                    for key_url in repo.gpg_key_urls
                ],
            )
            for repo in yc.gen_repos()
        )
        .isolate_main(
            config_path=(install_dir / config_path).decode(),
            pluginconf_dir=(install_dir / plugin_dir).decode(),
            cache_dir=(install_dir / cache_dir).decode(),
        )
    )
    with create_ro(out_dir / config_path, "w") as conf_out:
        isolated_yc.write(conf_out)


def main(argv: List[str]) -> None:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--rpm-installer", required=True, type=YumDnf)
    parser.add_argument(
        "--input-conf",
        required=True,
        type=Path.from_argparse,
        help="The `yum` or `dnf` config file to rewrite",
    )
    parser.add_argument(
        "--output-dir",
        required=True,
        type=Path.from_argparse,
        help="Write new configs here, part of the snapshot build artifact",
    )
    parser.add_argument(
        "--install-dir",
        required=True,
        type=Path.from_argparse,
        help="In the container, `--output-dir` will be available here",
    )
    parser.add_argument(
        "--repo-server-ports",
        required=True,
        help="The rewritten config will direct the RPM installer to talk to "
        "`repo-server` proxies listenting on `localhost:<PORT>`s",
    )
    args = Path.parse_args(parser, argv)

    with populate_temp_dir_and_rename(args.output_dir) as td, open(
        args.input_conf, "r"
    ) as infile:
        write_yum_dnf_conf(
            yum_dnf=args.rpm_installer,
            infile=infile,
            out_dir=td,
            install_dir=args.install_dir,
            ports=[int(p) for p in args.repo_server_ports.split()],
        )


if __name__ == "__main__":  # pragma: no cover
    import sys

    main(sys.argv[1:])
