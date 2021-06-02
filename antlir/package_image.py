#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""\
Serialize a btrfs subvolume built by an `image_layer` target into a
portable format (either a file, or a directory with a few files).

At the moment, this only outputs "full" packages -- that is, we do not
support emitting an incremental package relative to a prior `image_layer`.

## How to add support for incremental packages

There is a specific setting, where it is possible to support safe
incremental packaging.  First, read on to understand why the general case of
incremental packaging is intrinsically unsafe.

### The incremental package consistency problem

It is technically simple to create incremental outputs:
  - `btrfs send -p`
  - `tar --create --listed-incremental`

The problem is that it is hard to guarantee consistency between parts of the
incremental stack.

It is reasonable for an end-user to expect this to work correctly, so long
as they build both parts from excatly the same source control version:
 - first, they build package A;
 - later (perhaps on a different host or repo checkout), they build an
   incremental package B that stacks on top of A.

Indeed, this generally works for programming artifacts, because programming
languages define a clear interface for their build artifacts, and the same
source code + build toolchain is GUARANTEED to always produce artifacts that
are interface-compatible with other outputs from the same inputs.

In contrast, a filesystem output of an image build does NOT define such an
interface, which makes it impossible to guarantee consistency.  Let's make
this concrete with an example.

Imagine these Buck targets:
 - `:parent_subvol`
 - `:child_subvol`, with `parent_layer = ":parent_subvol"`

Let's say that `:parent_subvol` contains, among other things, a multi-file
relational DB which stores a table per file, and uses RANDOM keys
internally. The first time we build it, we might get this:

```
$ jq . table_names
{
    "randKeyA3": {"name": "cat"},
    "randKeyA1": {"name": "dog"},
    "randKeyA8": {"name": "gibbon"}
}
$ jq . table_friends
{
    "randKeyA3": ["randKeyA1"]
}
```

This database just says that we have 3 animals, and 1 directed friendship
among them (cat -> dog).

You can imagine a second build of `:parent_subvol` which has the same
semantic content:

```
$ jq . table_names
{
    "randKeyA6": {"name": "cat"},
    "randKeyA5": {"name": "dog"},
    "randKeyA1": {"name": "gibbon"}
}
$ jq . table_friends
{
    "randKeyA6": ["randKeyA5"]
}
```

Since the random keys are internal to the DB, and not part of its public
API, this is permissible build entropy -- just like "build info" sections in
binary objects, and just like build timestamps.

So from the point of view of Buck, everything is fine.

Now, let's say that in `:child_subvol` we add another friendship to the DB
(gibbon -> dog).  Depending on the version of `:parent_subvol` you start
with, building `:child_subvol` will cause you to produce an incremental
package replaceing JUST the file `table_friends` with one of these versions:

```
# `:child_subvol` from the first `:parent_subvol` build
$ jq . table_friends
{
    "randKeyA3": ["randKeyA1"],
    "randKeyA8": ["randKeyA1"]
}
# `:child_subvol` from the second `:parent_subvol` build
$ jq . table_friends
{
    "randKeyA6": ["randKeyA5"],
    "randKeyA1": ["randKeyA5"],
}
```

Omitting `table_names` from the incremental update is completely fine from
the point of view of the filesystem -- that file hasn't changed in either
build.  However, we now have two INCOMPATIBLE build artifacts from the same
source version.

Now, we may end up combining the first version of `:parent_subvol` with the
second version of `:child_subvol`. The incremental update would apply fine,
but the resulting DB would be corrupted.

Worst of all, this could occur quite naturally, e.g.
  - An innocent (but not stupid!) user may assume that since builds are
    hermetic, build artifacts from the same version are compatible.
  - Target-level distributed caching in Buck may cache artifacts from two
    different build runs.  On the Buck side, T35569915 documents the
    intention to make ALL cache retrievals be based only on input keys,
    which could actually guarantee the consistency we need, but this is
    probably not happening before late 2019, early 2020.

To sum up:

 - In practice, builds are almost never bitwise-reproducible. The resulting
   filesystem contents of two builds of the same repo state may differ.
   When we say a build environment is hermetic we just mean that at runtime,
   all of its artifacts work the same way, so long as they were built from
   the same repo state.

 - Filesystems lack a standard semantic interface, which could guarantee
   interoperability between filesystem artifacts from two differen builds of
   the same "hermetic" environment.  Therefore, any kind of "incremental"
   package has to be applied against EXACTLY the same filesystem contents,
   against which it was built, or the result may be incorrect.

 - In a distributed build setting, it's hard to guarantee that incremental
   build artifacts will NOT get composed incorrectly.

 - So, we choose NOT to support incremental packaging in the general case.
   We may revise this decision once Buck's cache handling changes
   (T35569915), or if the need for incremental packaging is strong enough to
   justify shipping solutions with razor-sharp edges.

### When can we safely build incremental packages?

Before getting to the practically useful solution, let me mention a
less-useful one in passing.  It is simple to define a rule type that outputs
a STACK of known-compatible incremental packages.  The current code has
commented-out breadcrumbs (see `get_subvolume_on_disk_stack`), while
P60233442 adds ~20 lines of code to materializing an incremental send-stream
stack.  This solves the consistency problem, but it's unclear what value
this type of rule provides over a "full" package.

The main use-case for incremental builds is this:
 - pieces of widely-used infrastructure are packaged up into a few
   common base images,
 - custom container images are distributed as incremental add-ons to these
   common bases.

In this case, we can side-step the above correctness issues by requiring
that any base `image_layer` for an incremental package must have a "release"
property.  This is an assertion that can be verified at build-time, stating
that a content hash of the base layer has been checked into the source
control repo.  While the production version of this might look a little
different, this demonstrates the right semantics:

```
$ cat TARGETS
buck_genrule(
    name='parent.sendstream',
    out='parent.sendstream',
    bash='... fetch the sendstream from some blob store ...',
)

image_sendstream_layer(
    name='parent',
    source=':parent.sendstream',
    # The presence of this hash assures us that the filesystem contents are
    # fixed, which makes it safe to build incremental snapshots against it.
    sendstream_hash={
        'sha256':
            '4449df7d6848198f310aaffa7f7860b6022017e1913b94b6af86bb618e999480',
    },
)

image_layer(
    name='child',
    parent_layer=':parent',
    ...
)

image_package(
    name='child_from_parent.sendstream',
    layer=':child',
    # If `:parent` lacked `sendstream_hash`, we would not know it is a
    # "release" image, and this `image_package` would fail to build.
    incremental_to=':parent',
)
```

Besides tweaks to naming, the main difference I would expect in a production
system is a more automatable way of specifying content hashes for previously
released base images.

Requiring base images to be released adds some conceptual complexity. However,
it is quite reasonable to have post-CI release processes for commonly used
base images. Specific advantages to this include:
 - more rigorous testing than is feasible in at-code-review-time CI/CD system
 - the ability to pre-warm caches, thus ensuring nearly instant availability
   of the base images.
"""
import argparse
import os
import pwd
import stat
import subprocess
from typing import Mapping, NamedTuple, Optional, Callable

from antlir.nspawn_in_subvol.args import PopenArgs, new_nspawn_opts
from antlir.nspawn_in_subvol.nspawn import popen_nspawn, run_nspawn

from .common import check_popen_returncode, init_logging
from .find_built_subvol import find_built_subvol
from .fs_utils import Path, create_ro, generate_work_dir
from .subvol_utils import Subvol, SubvolOpts, KiB, MiB


class _Opts(NamedTuple):
    subvol_opts: SubvolOpts
    build_appliance: Optional[Subvol]
    size_mb: Optional[int]
    volume_label: Optional[str]


class Format:
    "A base class that registers its subclasses in NAME_TO_CLASS."

    NAME_TO_CLASS: Mapping[str, "Format"] = {}

    def __init_subclass__(cls, format_name: str, **kwargs):
        super().__init_subclass__(**kwargs)
        prev_cls = cls.NAME_TO_CLASS.get(format_name)
        if prev_cls:
            raise AssertionError(f"{cls} and {prev_cls} share format_name")
        cls.NAME_TO_CLASS[format_name] = cls

    @classmethod
    def make(cls, format_name) -> "Format":
        return cls.NAME_TO_CLASS[format_name]()


class Sendstream(Format, format_name="sendstream"):
    """
    Packages the subvolume as a stand-alone (non-incremental) send-stream.
    See the script-level docs for details on supporting incremental ones.
    """

    def package_full(self, subvol: Subvol, output_path: str, opts: _Opts):
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

    def package_full(self, subvol: Subvol, output_path: str, opts: _Opts):
        with create_ro(output_path, "wb") as outfile, subprocess.Popen(
            ["zstd", "--stdout"], stdin=subprocess.PIPE, stdout=outfile
        ) as zst, subvol.mark_readonly_and_write_sendstream_to_file(zst.stdin):
            pass
        check_popen_returncode(zst)


class SquashfsImage(Format, format_name="squashfs"):
    """
    Packages the subvolume as a squashfs-formatted disk image, usage:
      mount -t squashfs image.squashfs dest/ -o loop
    """

    def package_full(self, subvol: Subvol, output_path: str, opts: _Opts):
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


class BtrfsImage(Format, format_name="btrfs"):
    """
    Packages the subvolume as a btrfs-formatted disk image, usage:
      mount -t btrfs image.btrfs dest/ -o loop
    """

    def package_full(self, subvol: Subvol, output_path: str, opts: _Opts):
        subvol.mark_readonly_and_send_to_new_loopback(
            output_path, subvol_opts=opts.subvol_opts
        )


class TarballGzipImage(Format, format_name="tar.gz"):
    """
    Packages the subvolume as a gzip-compressed tarball, usage:
      tar xzf image.tar.gz -C dest/
    """

    def package_full(self, subvol: Subvol, output_path: str, opts: _Opts):
        with create_ro(output_path, "wb") as outfile, subprocess.Popen(
            ["gzip", "--stdout"], stdin=subprocess.PIPE, stdout=outfile
        ) as gz, subvol.write_tarball_to_file(gz.stdin):
            pass

        check_popen_returncode(gz)


class CPIOGzipImage(Format, format_name="cpio.gz"):
    """
    Packages the subvol as a gzip-compressed cpio.
    """

    def package_full(self, subvol: Subvol, output_path: str, opts: _Opts):
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

        with create_ro(output_path, "wb") as outfile, popen_nspawn(
            opts, PopenArgs(stdout=outfile)
        ):
            pass


def _bash_cmd_in_build_appliance(
    output_path: str,
    opts: _Opts,
    subvol: Subvol,
    get_bash: Callable[[str, str], str],
):
    """
    Spin up a new nspawn build appliance with bind mounts
    and run cmd provided by get_bash.
    """
    work_dir = generate_work_dir()
    output_dir = "/output"
    o_basepath, o_file = os.path.split(output_path)
    image_path = os.path.join(output_dir, o_file)
    cmd = [
        "/bin/bash",
        "-eux",
        "-o",
        "pipefail",
        "-c",
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
            user=pwd.getpwuid(os.getuid()),
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

    def package_full(self, subvol: Subvol, output_path: str, opts: _Opts):
        if opts.size_mb is None:
            raise ValueError("size_mb is required when packaging a vfat image")
        _bash_cmd_in_build_appliance(
            output_path,
            opts,
            subvol,
            lambda *, image_path, work_dir: (
                "/usr/sbin/mkfs.vfat {maybe_label} "
                "-C {image_path} {image_size}; "
                "/usr/bin/mcopy -v -i {image_path} -sp {work_dir}/* ::"
            ).format(
                maybe_label=f"-n {opts.volume_label}"
                if opts.volume_label
                else "",
                image_path=image_path,
                # mkfs.vfat takes the size as BLOCK_COUNT (1K Bytes)
                # thus passing in "size_mb * KiB" results in "size_mb" MiB
                image_size=opts.size_mb * KiB,
                work_dir=work_dir,
            ),
        )


class Ext3Image(Format, format_name="ext3"):
    """
    Packages the subvolume as an ext3-formatted disk image, usage:
      mount -t ext3 image.ext3 dest/ -o loop
    """

    def package_full(self, subvol: Subvol, output_path: str, opts: _Opts):
        if opts.size_mb is None:
            raise ValueError("size_mb is required when packaging an ext3 image")
        _bash_cmd_in_build_appliance(
            output_path,
            opts,
            subvol,
            lambda *, image_path, work_dir: (
                "/usr/bin/truncate -s {image_size}M {image_path}; "
                "/usr/sbin/mkfs.ext3 {maybe_label} {image_path}"
                " -d {work_dir}"
            ).format(
                maybe_label=f"-L {opts.volume_label}"
                if opts.volume_label
                else "",
                image_path=image_path,
                image_size=opts.size_mb,
                work_dir=work_dir,
            ),
        )


def parse_args(argv):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--subvolumes-dir",
        required=True,
        type=Path.from_argparse,
        help="A directory on a btrfs volume, where all the subvolume wrapper "
        "directories reside.",
    )
    parser.add_argument(
        "--layer-path",
        required=True,
        help="A directory output from the `image_layer` we need to package",
    )
    parser.add_argument(
        "--format",
        choices=Format.NAME_TO_CLASS.keys(),
        required=True,
        help=f"""
        Brief format descriptions -- see the code docblocks for more detail:
            {'; '.join(
                '"' + k + '" -- ' + v.__doc__
                    for k, v in Format.NAME_TO_CLASS.items()
            )}
        """,
    )
    parser.add_argument(
        "--output-path",
        required=True,
        help="Write the image package file(s) to this path -- must not exist",
    )
    parser.add_argument(
        "--writable-subvolume",
        action="store_true",
        default=False,
        help="By default, the subvolume inside a loopback is marked read-only."
        " Pass this flag to mark it writable.",
    )
    parser.add_argument(
        "--seed-device",
        action="store_true",
        default=False,
        help="Pass this flag to make the resulting image a btrfs seed device",
    )
    parser.add_argument(
        "--size-mb",
        type=int,
        help="Size of the target filesystem image",
    )
    parser.add_argument(
        "--volume-label",
        help="Label for the target filesystem image",
    )
    parser.add_argument(
        "--multi-pass-size-minimization",
        action="store_true",
        default=False,
        help="By default, we do not apply time-costly efforts to minimize the"
        " size of loopback image",
    )
    parser.add_argument(
        "--set-default-subvol",
        action="store_true",
        default=False,
        help="Set the default subvol of the loopback image to be the volume:"
        " subvol",
    )
    parser.add_argument(
        "--build-appliance", help="Build appliance layer to use when packaging"
    )
    # Future: To add support for incremental send-streams, we'd want to
    # use this (see `--ancestor-jsons` in `image_package.bzl`)
    #
    # parser.add_argument(
    #     '--ancestor-jsons',
    #     nargs=argparse.REMAINDER, metavar=['PATH'], required=True,
    #     help='Consumes the remaining arguments on the command-line. '
    #         'A list of image_layer JSON output files.',
    # )
    return Path.parse_args(parser, argv)


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


def package_image(argv):
    args = parse_args(argv)
    assert not os.path.exists(args.output_path)
    Format.make(args.format).package_full(
        find_built_subvol(args.layer_path, subvolumes_dir=args.subvolumes_dir),
        output_path=args.output_path,
        opts=_Opts(
            subvol_opts=SubvolOpts(
                readonly=not args.writable_subvolume,
                seed_device=args.seed_device,
                set_default_subvol=args.set_default_subvol,
                multi_pass_size_minimization=args.multi_pass_size_minimization,
                size_bytes=args.size_mb * MiB if args.size_mb else None,
            ),
            build_appliance=find_built_subvol(args.build_appliance)
            if args.build_appliance
            else None,
            size_mb=args.size_mb,
            volume_label=args.volume_label,
        ),
    )
    # Paranoia: images are read-only after being built
    os.chmod(
        args.output_path,
        stat.S_IMODE(os.stat(args.output_path).st_mode)
        & ~(stat.S_IWUSR | stat.S_IWGRP | stat.S_IWOTH),
    )


if __name__ == "__main__":  # pragma: no cover
    import sys

    init_logging()
    package_image(sys.argv[1:])
