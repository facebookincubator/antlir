#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import pwd
import subprocess
from typing import AnyStr, Callable, Mapping, NamedTuple, Optional

from antlir.cli import (
    add_targets_and_outputs_arg,
    init_cli,
    normalize_buck_path,
)
from antlir.config import repo_config
from antlir.nspawn_in_subvol.args import PopenArgs, new_nspawn_opts
from antlir.nspawn_in_subvol.nspawn import popen_nspawn, run_nspawn

from .bzl.loopback_opts import loopback_opts_t
from .common import (
    check_popen_returncode,
    get_logger,
    pipe,
)
from .find_built_subvol import find_built_subvol
from .fs_utils import META_FLAVOR_FILE, Path, create_ro, generate_work_dir
from .loopback import BtrfsLoopbackVolume, MIN_CREATE_BYTES, MIN_FREE_BYTES
from .subvol_utils import Subvol
from .unshare import Unshare, Namespace

log = get_logger()
KiB = 2**10
MiB = 2**20


class _Opts(NamedTuple):
    build_appliance: Optional[Subvol]
    loopback_opts: loopback_opts_t


class Format:
    "A base class that registers its subclasses in NAME_TO_CLASS."

    NAME_TO_CLASS: Mapping[str, "Format"] = {}

    def __init_subclass__(cls, format_name: str, **kwargs) -> None:
        super().__init_subclass__(**kwargs)
        prev_cls = cls.NAME_TO_CLASS.get(format_name)
        if prev_cls:
            raise AssertionError(f"{cls} and {prev_cls} share format_name")
        # pyre-fixme[16]: `Mapping` has no attribute `__setitem__`.
        cls.NAME_TO_CLASS[format_name] = cls

    @classmethod
    def make(cls, format_name: str) -> "Format":
        # pyre-fixme[29]: `Format` is not a function.
        return cls.NAME_TO_CLASS[format_name]()


class Sendstream(Format, format_name="sendstream"):
    """
    Packages the subvolume as a stand-alone (non-incremental) send-stream.
    See the script-level docs for details on supporting incremental ones.
    """

    def package_full(
        self, subvol: Subvol, output_path: Path, opts: _Opts
    ) -> None:
        with create_ro(
            output_path, "wb"
        ) as outfile, subvol.mark_readonly_and_write_sendstream_to_file(
            outfile
        ):
            pass


class SendstreamZst(Format, format_name="sendstream.zst"):
    """
    Packages the subvolume as a stand-alone (non-incremental) zstd-compressed
    send-stream. See the script-level docs for details on supporting incremental
    ones.
    Future: add general compression support instead of adding `TarballGz`,
    `TarballZst`, `SendstreamGz`, etc.
    """

    def package_full(
        self, subvol: Subvol, output_path: Path, opts: _Opts
    ) -> None:
        with create_ro(output_path, "wb") as outfile, subprocess.Popen(
            ["zstd", "--stdout"],
            stdin=subprocess.PIPE,
            stdout=outfile
            # pyre-fixme[6]: Expected `BinaryIO` for 1st param but got
            #  `Optional[typing.IO[typing.Any]]`.
        ) as zst, subvol.mark_readonly_and_write_sendstream_to_file(zst.stdin):
            pass
        check_popen_returncode(zst)


class SquashfsImage(Format, format_name="squashfs"):
    """
    Packages the subvolume as a squashfs-formatted disk image, usage:
      mount -t squashfs image.squashfs dest/ -o loop
    """

    def package_full(
        self, subvol: Subvol, output_path: str, opts: _Opts
    ) -> None:
        create_ro(output_path, "wb").close()  # Ensure non-root ownership
        subvol.run_as_root(
            [
                "mksquashfs",
                subvol.path(),
                output_path,
                "-comp",
                "zstd",
                "-noappend",
            ]
        )


class TarballGzipImage(Format, format_name="tar.gz"):
    """
    Packages the subvolume as a gzip-compressed tarball, usage:
      tar xzf image.tar.gz -C dest/
    """

    def package_full(
        self, subvol: Subvol, output_path: str, opts: _Opts
    ) -> None:
        with create_ro(output_path, "wb") as outfile, subprocess.Popen(
            ["gzip", "--stdout"],
            stdin=subprocess.PIPE,
            stdout=outfile
            # pyre-fixme[6]: Expected `BinaryIO` for 1st param but got
            #  `Optional[typing.IO[typing.Any]]`.
        ) as gz, subvol.write_tarball_to_file(gz.stdin):
            pass

        check_popen_returncode(gz)


class CPIOGzipImage(Format, format_name="cpio.gz"):
    """
    Packages the subvol as a gzip-compressed cpio.
    """

    def package_full(
        self, subvol: Subvol, output_path: str, opts: _Opts
    ) -> None:
        work_dir = generate_work_dir()

        # This command is partly based on the recomendations of
        # reproducible-builds.org:
        # https://reproducible-builds.org/docs/archives/
        # Note that this does *not* create a reproducible archive yet.
        # For that we need 2 more things:
        #   - Clearing of the timestamps
        #   - Setting uid/gid to 0
        # Those 2 operations mutate the filesystem.  Packaging
        # should be transparent and not cause mutations, as such
        # those operations should be added as genrule layers (or
        # something similar) that mutates the filesystem being
        # packaged *before* reaching this point.
        create_archive_cmd = [
            "/bin/bash",
            "-c",
            "set -ue -o pipefail;" f"pushd {work_dir} >/dev/null;"
            # List all the files except sockets since cpio doesn't
            # support them and they don't really mean much outside
            # the context of the process that is using it.
            "(set -ue -o pipefail; /bin/find . -mindepth 1 ! -type s | "
            # Use LANG=C to avoid any surprises that locale might cause
            "LANG=C /bin/sort | "
            # Create the archive with bsdtar
            "LANG=C /bin/cpio -o -H newc |"
            # And finally compress it
            "/bin/gzip --stdout)",
        ]

        opts = new_nspawn_opts(
            cmd=create_archive_cmd,
            layer=opts.build_appliance,
            bindmount_rw=[(subvol.path(), work_dir)],
            user=pwd.getpwnam("root"),
        )

        # pyre-fixme[16]: `Iterable` has no attribute `__enter__`.
        with create_ro(output_path, "wb") as outfile, popen_nspawn(
            opts, PopenArgs(stdout=outfile)
        ):
            pass


def _bash_cmd_in_build_appliance(
    output_path: Path,
    opts: _Opts,
    subvol: Subvol,
    get_bash: Callable[[str, str], str],
) -> None:
    """
    Spin up a new nspawn build appliance with bind mounts
    and run cmd provided by get_bash.
    """

    # create the output file first so it's owned by the current user.
    create_ro(output_path, "wb").close()  # Ensure non-root ownership

    work_dir = generate_work_dir()
    output_dir = Path("/output")
    o_basepath, o_file = os.path.split(output_path)
    image_path = output_dir / o_file
    cmd = [
        "/bin/bash",
        "-eux",
        "-o",
        "pipefail",
        "-c",
        # pyre-fixme[28]: Unexpected keyword argument `image_path`.
        get_bash(image_path=image_path, work_dir=work_dir),
    ]
    run_nspawn(
        new_nspawn_opts(
            cmd=cmd,
            layer=opts.build_appliance,
            bindmount_rw=[
                (subvol.path(), work_dir),
                (o_basepath, output_dir),
            ],
            # Run as root so we can access files owned by different users.
            user=pwd.getpwnam("root"),
        ),
        PopenArgs(),
    )


class VfatImage(Format, format_name="vfat"):
    """
    Packages the subvolume as a vfat-formatted disk image, usage:
      mount -t vfat image.vfat dest/ -o loop
    NB: vfat is very limited on supported file types, thus we only support
    packaging regular files/dirs into a vfat image.
    """

    def package_full(
        self, subvol: Subvol, output_path: Path, opts: _Opts
    ) -> None:
        if opts.loopback_opts.size_mb is None:
            raise ValueError(
                "loopback_opts.size_mb is required when packaging a vfat image"
            )
        _bash_cmd_in_build_appliance(
            output_path,
            opts,
            subvol,
            # pyre-fixme[6]: Expected `(str, str) -> str` for 4th param but got
            #  `(image_path: Any, work_dir: Any) -> str`.
            lambda *, image_path, work_dir: (
                "/usr/bin/truncate -s {image_size_mb}M {image_path}; "
                "/usr/sbin/mkfs.vfat {maybe_fat_size} {maybe_label} "
                "{image_path}; "
                "/usr/bin/mcopy -v -i {image_path} -sp {work_dir}/* ::"
            ).format(
                maybe_fat_size=f"-F{opts.loopback_opts.fat_size}"
                if opts.loopback_opts.fat_size
                else "",
                maybe_label=f"-n {opts.loopback_opts.label}"
                if opts.loopback_opts.label
                else "",
                image_path=image_path,
                image_size_mb=opts.loopback_opts.size_mb,
                work_dir=work_dir,
            ),
        )


class Ext3Image(Format, format_name="ext3"):
    """
    Packages the subvolume as an ext3-formatted disk image, usage:
      mount -t ext3 image.ext3 dest/ -o loop
    """

    def package_full(
        self, subvol: Subvol, output_path: Path, opts: _Opts
    ) -> None:
        if opts.loopback_opts.size_mb is None:
            raise ValueError(
                "loopback_opts.size_mb is required when packaging an ext3 image"
            )
        _bash_cmd_in_build_appliance(
            output_path,
            opts,
            subvol,
            # pyre-fixme[6]: Expected `(str, str) -> str` for 4th param but got
            #  `(image_path: Any, work_dir: Any) -> str`.
            lambda *, image_path, work_dir: (
                "/usr/bin/truncate -s {image_size_mb}M {image_path}; "
                "/usr/sbin/mkfs.ext3 {maybe_label} {image_path}"
                " -d {work_dir}"
            ).format(
                maybe_label=f"-L {opts.loopback_opts.label}"
                if opts.loopback_opts.label
                else "",
                image_path=image_path,
                image_size_mb=opts.loopback_opts.size_mb,
                work_dir=work_dir,
            ),
        )


# Future: For incremental snapshots, an important sanity check is to verify
# that base subvolume is actually an ancestor of the subvolume being
# packaged, since `btrfs send` does not check this.  The function below
# enables us to do this, and more.
#
# def get_subvolume_on_disk_stack(
#     layer_json_paths: Iterable[str], subvolumes_dir: str,
# ) -> List[SubvolumeOnDisk]:
#     # Map the given layer JSONs to btrfs subvolumes in the per-repo volume
#     uuid_to_svod = {}
#     parent_uuids = set()
#     for json_path in layer_json_paths:
#         with open(json_path) as infile:
#             svod = SubvolumeOnDisk.from_json_file(infile, subvolumes_dir)
#             uuid_to_svod[svod.btrfs_uuid] = svod
#             if svod.btrfs_parent_uuid:
#                 parent_uuids.add(svod.btrfs_parent_uuid)
#
#     # Traverse `SubvolumeOnDisk`s from the leaf child to the last ancestor
#     svod, = (s for u, s in uuid_to_svod.items() if u not in parent_uuids)
#     subvol_stack = []
#     while True:
#         subvol_stack.append(svod)
#         if not svod.btrfs_parent_uuid:
#             break
#         svod = uuid_to_svod[svod.btrfs_parent_uuid]
#     subvol_stack.reverse()  # Now from last ancestor to newest child
#     assert len(subvol_stack) == len(uuid_to_svod), uuid_to_svod
#     assert len(set(subvol_stack)) == len(uuid_to_svod), uuid_to_svod
#
#     return subvol_stack


def _get_build_appliance_from_layer_flavor_config(
    layer: Subvol, targets_and_outputs: Mapping[AnyStr, Path]
) -> Path:
    return targets_and_outputs[
        repo_config()
        .flavor_to_config[layer.read_path_text(META_FLAVOR_FILE)]
        .build_appliance
    ]


def package_image(args) -> None:
    with init_cli(description=__doc__, argv=args) as cli:
        cli.parser.add_argument(
            "--subvolumes-dir",
            required=True,
            type=Path.from_argparse,
            help="A directory on a btrfs volume, where all the subvolume "
            "wrapper directories reside.",
        )
        cli.parser.add_argument(
            "--layer-path",
            required=True,
            help="A directory output from the `image_layer` we need to package",
        )
        cli.parser.add_argument(
            "--format",
            choices=Format.NAME_TO_CLASS.keys(),
            required=True,
            # pyre-fixme[58]: `+` is not supported for operand types `str` and
            #  `Optional[str]`.
            help=f"""
            Brief format descriptions -- see the code docblocks for more detail:
                {'; '.join(
                    '"' + k + '" -- ' + v.__doc__
                        for k, v in Format.NAME_TO_CLASS.items()
                )}
            """,
        )
        cli.parser.add_argument(
            "--output-path",
            required=True,
            type=normalize_buck_path,
            help="Write the image package file(s) to this path. This "
            "path must not already exist.",
        )
        cli.parser.add_argument(
            "--loopback-opts",
            type=loopback_opts_t.parse_raw,
            default=loopback_opts_t(),
            help="Inline serialized loopback_opts_t instance containing "
            "configuration options for loopback formats",
        )

        add_targets_and_outputs_arg(cli.parser)

        # Future: To add support for incremental send-streams, we'd want to
        # use this (see `--ancestor-jsons` in `image/package/new.bzl`)
        #
        # parser.add_argument(
        #     '--ancestor-jsons',
        #     nargs=argparse.REMAINDER, metavar=['PATH'], required=True,
        #     help='Consumes the remaining arguments on the command-line. '
        #         'A list of image_layer JSON output files.',
        # )

    # Buck should remove this path if the target needs to be rebuilt.
    # This is a safety check to make sure we're not doing anything behind buck's
    # back.
    assert not cli.args.output_path.exists()

    layer = find_built_subvol(
        cli.args.layer_path, subvolumes_dir=cli.args.subvolumes_dir
    )

    build_appliance = find_built_subvol(
        _get_build_appliance_from_layer_flavor_config(
            layer=layer, targets_and_outputs=cli.args.targets_and_outputs
        )
    )

    # pyre-fixme[16]: `Format` has no attribute `package_full`.
    Format.make(cli.args.format).package_full(
        output_path=cli.args.output_path,
        opts=_Opts(
            build_appliance=build_appliance,
            loopback_opts=cli.args.loopback_opts,
        ),
        subvol=layer,
    )


# This is covered by integration tests using `package.bzl`
if __name__ == "__main__":  # pragma: no cover
    package_image(None)
