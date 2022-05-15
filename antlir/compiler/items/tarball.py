#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import pwd

from antlir.bzl.image.feature.tarball import tarball_t
from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesFile,
    ProvidesSymlink,
    RequireDirectory,
)
from antlir.fs_utils import generate_work_dir, open_for_read_decompress, Path
from antlir.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.subvol_utils import Subvol

from .common import (
    ImageItem,
    LayerOpts,
    make_path_normal_relative,
    validate_path_field_normal_relative,
)


def load_from_tarball(
    source: str,
    subvol: Subvol,
    layer_opts: LayerOpts,
    into_dir=None,
    force_root_ownership: bool = False,
) -> None:
    into_dir = into_dir or Path("")

    build_appliance = layer_opts.requires_build_appliance()
    work_dir = generate_work_dir()
    tar_cmd = " ".join(
        [
            "tar",
            # Future: Bug: `tar` unfortunately FOLLOWS existing symlinks
            # when unpacking.  This isn't dire because the compiler's
            # conflict prevention SHOULD prevent us from going out of
            # the subvolume since this TarballItem's provides would
            # collide with whatever is already present.  However, it's
            # hard to state that with complete confidence, especially if
            # we start adding support for following directory symlinks.
            "--directory",
            (work_dir / into_dir).decode(),
            "--extract",
            # preserving xattrs need to be specified on both sides (packing
            # and unpacking)
            "--acls",
            "--xattrs",
            # Block tar's weird handling of paths containing colons.
            "--force-local",
            # The uid:gid doing the extraction is root:root, so by default
            # tar would try to restore the file ownership from the archive.
            # In some cases, we just want all the files to be root-owned.
            *(["--no-same-owner"] if force_root_ownership else []),
            # The next option is an extra safeguard that is redundant
            # with the compiler's prevention of `provides` conflicts.
            # It has two consequences:
            #
            #  (1) If a file already exists, `tar` will fail with an error.
            #      It is **not** an error if a directory already exists --
            #      otherwise, one would never be able to safely untar
            #      something into e.g. `/usr/local/bin`.
            #
            #  (2) Less obviously, the option prevents `tar` from
            #      overwriting the permissions of `directory`, as it
            #      otherwise would.
            #
            #      Thanks to the compiler's conflict detection, this should
            #      not come up, but now you know.  Observe us clobber the
            #      permissions without it:
            #
            #        $ mkdir IN OUT
            #        $ touch IN/file
            #        $ chmod og-rwx IN
            #        $ ls -ld IN OUT
            #        drwx------. 2 lesha users 17 Sep 11 21:50 IN
            #        drwxr-xr-x. 2 lesha users  6 Sep 11 21:50 OUT
            #        $ tar -C IN -czf file.tgz .
            #        $ tar -C OUT -xvf file.tgz
            #        ./
            #        ./file
            #        $ ls -ld IN OUT
            #        drwx------. 2 lesha users 17 Sep 11 21:50 IN
            #        drwx------. 2 lesha users 17 Sep 11 21:50 OUT
            #
            #      Adding `--keep-old-files` preserves `OUT`'s metadata:
            #
            #        $ rm -rf OUT ; mkdir out ; ls -ld OUT
            #        drwxr-xr-x. 2 lesha users 6 Sep 11 21:53 OUT
            #        $ tar -C OUT --keep-old-files -xvf file.tgz
            #        ./
            #        ./file
            #        $ ls -ld IN OUT
            #        drwx------. 2 lesha users 17 Sep 11 21:50 IN
            #        drwxr-xr-x. 2 lesha users 17 Sep 11 21:54 OUT
            "--keep-old-files",
            "--file",
            "-",
        ]
    )
    # pyre-fixme[6]: For 1st param expected `Path` but got `str`.
    with open_for_read_decompress(source) as tf:
        opts = new_nspawn_opts(
            # '0<&3' below redirects fd=3 to stdin, so 'tar ... -f -' will
            # read and unpack whatever we represent as fd=3. We pass `tf` as
            # fd=3 into container by 'forward_fd=...' below. See help
            # string in antlir/nspawn_in_subvol/args.py where
            # _parser_add_nspawn_opts() calls
            # parser.add_argument('--forward-fd')
            cmd=["sh", "-uec", f"{tar_cmd} 0<&3"],
            layer=build_appliance,
            bindmount_rw=[(subvol.path(), work_dir)],
            user=pwd.getpwnam("root"),
            forward_fd=[tf.fileno()],
            allow_mknod=True,
        )
        run_nspawn(opts, PopenArgs())


# pyre-fixme[13]: Attribute `source` is never initialized.
class TarballItem(tarball_t, ImageItem):
    source: Path

    _normalize_into_dir = validate_path_field_normal_relative("into_dir")

    def provides(self):
        # We own ZST decompression, tarfile handles other gz, bz2, etc.
        import tarfile  # Lazy since only this method needs it.

        with open_for_read_decompress(self.source) as tf, tarfile.open(
            fileobj=tf, mode="r|*"
        ) as f:
            for item in f:
                path = self.into_dir / make_path_normal_relative(
                    # This checks that the tarball doesn't write outside
                    # of `into_dir`, but `meta_check` doesn't make sense
                    # since it would prevent user tarballs from writing e.g.
                    # to `/my_tarball/.meta`, which is not protected.
                    #
                    # If `into_dir` is `/` and you have `/.meta`, you
                    # probably want to be using `image.layer_from_package`.
                    item.name,
                    meta_check=False,
                )
                if item.isdir():
                    # We do NOT provide the installation directory, and the
                    # image build script tarball extractor takes pains (e.g.
                    # `tar --no-overwrite-dir`) not to touch the extraction
                    # directory.
                    if path.relpath(self.into_dir).normpath() != b".":
                        yield ProvidesDirectory(path=path)
                elif item.issym():
                    yield ProvidesSymlink(path=path, target=Path(item.linkname))
                else:
                    yield ProvidesFile(path=path)

    def requires(self):
        yield RequireDirectory(path=self.into_dir)

    def build(self, subvol: Subvol, layer_opts: LayerOpts) -> None:
        load_from_tarball(
            # pyre-fixme[6]: Expected `str` for 1st param but got `Path`.
            self.source,
            subvol,
            layer_opts,
            into_dir=self.into_dir,
            # pyre-fixme[6]: For 5th param expected `bool` but got `Optional[bool]`.
            force_root_ownership=self.force_root_ownership,
        )
