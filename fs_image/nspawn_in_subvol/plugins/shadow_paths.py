#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Shadow certain paths inside the container with other container paths, by
adding this to `plugins` kwarg of the `run_*` or `popen_*` functions:
  `ShadowPaths(shadow_paths)`

In practice, you will want `rpm_nspawn_plugins` instead.

One application of path shadowing is to (transiently) replace the system's
package manager with one that talks to our deterministic repo snaphot.

To allow in-place upgrades of the package managers, we set up shadow
copies that are compatible with the `LD_PRELOAD=librename_shadowed.so` hack.
"""
import subprocess
import textwrap
from contextlib import contextmanager
from typing import Any, AnyStr, Iterable, List, Mapping, NamedTuple, Tuple

from fs_image.common import set_new_key
from fs_image.fs_utils import Path
from fs_image.nspawn_in_subvol.args import PopenArgs, _NspawnOpts
from fs_image.nspawn_in_subvol.common import DEFAULT_SEARCH_PATHS
from fs_image.nspawn_in_subvol.plugin_hooks import (
    _NspawnSetup,
    _NspawnSetupCtxMgr,
)
from fs_image.subvol_utils import Subvol

from . import NspawnPlugin


SHADOWED_PATHS_ROOT = Path("__fs_image__/shadowed")


def _shadow_search_dirs(setenv: Iterable[AnyStr]) -> Iterable[Path]:
    """
    Returns a container-absolute search path, which is the union of
    `DEFAULT_PATH` and any user-specified PATH.  We include `DEFAULT_PATH`
    for extra safety, since binaries in "well-known directories" are liable
    to be accessed even if the `PATH` was modified by the user.
    """
    search_dirs = []
    path_prefix = b"PATH="
    for k_v in setenv:
        k_v = Path(k_v)
        if k_v.startswith(path_prefix):
            for p in k_v[len(path_prefix) :].split(b"="):
                search_dirs.append(Path(p))
    search_dirs.extend(DEFAULT_SEARCH_PATHS)
    # Eagerly deduplicate, while preserving order -- our subsequent
    # candidate lookup is expensive.
    seen_search_dirs = set()
    for search_dir in search_dirs:
        assert search_dir.startswith(b"/"), f"Non-absolute PATH: {search_dir}"
        if search_dir not in seen_search_dirs:
            yield search_dir
        seen_search_dirs.add(search_dir)


def _nul_separated_tuples(n, data: bytes) -> List[Any]:
    "For `data` separated by NUL bytes, interpret it as a list of n-tuples."
    flat = data.split(b"\0")
    assert flat.pop() == b""  # remove the trailing \0
    assert len(flat) % n == 0, flat
    return [flat[i : i + n] for i in range(0, len(flat), n)]


class _ShadowCandidate(NamedTuple):
    host_dest: Path  # Host path to what would get shadowed
    host_src: Path  # Host path that does the shadowing
    input_dest: Path  # The original key from `shadow_paths`


def _resolve_to_canonical_shadow_paths(
    *,
    shadow_paths: Iterable[Tuple[Path, Path]],
    subvol: Subvol,
    search_dirs: List[Path],
) -> Mapping[Path, Path]:
    "Converts `ShadowPaths` inputs to symlink-free host absolute paths."
    assert search_dirs, search_dirs
    # Generate candidate absolute paths for resolving filenames by
    # walking our container `PATH`.
    #
    # Don't output a map to allow some <absolute dest path> to be equal
    # to "search_dir / <dest filename>".  We check for duplicates later.
    candidates = []
    unmatched_inputs = {}  # Checked below
    for dest, src in shadow_paths:
        # If `dest` has duplicates, we'll show the error for the first `src`
        unmatched_inputs[dest] = src
        if dest.startswith(b"/"):
            candidate_dests = [dest]
        else:
            # Not an absolute path? It's a filename to resolve via PATH.
            assert b"/" not in dest, f"Neither absolute nor filename: {dest}"
            candidate_dests = [search_dir / dest for search_dir in search_dirs]
        for candidate_dest in candidate_dests:
            candidates.append(
                _ShadowCandidate(
                    # Do not `realpath` here because this would fail to
                    # resolve symlinks which the repo user cannot access.
                    host_dest=subvol.path(candidate_dest),
                    host_src=subvol.path(src),
                    input_dest=dest,
                )
            )

    # Check existence & resolve to real paths as `root` because
    # otherwise we would not get the right result if the path included
    # any directories not accessible by the repo user.
    resolved_triples = _nul_separated_tuples(
        3,
        subvol.run_as_root(
            [
                "sh",
                "-c",
                "\n".join(
                    # If both the candidate destination, and the source exist,
                    # output them together with the the input dest so we can
                    # match it in `shadow_paths`.
                    textwrap.dedent(
                        f"""\
            dst=$(readlink -f {c.host_dest.shell_quote()}) &&
            src=$(readlink -f {c.host_src.shell_quote()}) &&
            test -f "$dst" -a -f "$src" &&
            printf '%s\\0%s\\0%s\\0' "$dst" {c.input_dest.shell_quote()} "$src"
            """
                    )
                    for c in candidates
                    # The trailing `true` means that we ignore errors from `test
                    # -e` but not e.g. if `/bin/sh` does no texist.
                )
                + "\ntrue",
            ],
            stdout=subprocess.PIPE,
        ).stdout,
    )

    container_dest_to_real_src = {}
    subvol_prefix = subvol.path().realpath() + b"/"
    real_srcs = set()  # Do not let sources be used with multiple dests
    for real_dest, input_dest, real_src in resolved_triples:
        # We need the `None` because, due to symlinks, multiple inputs may
        # resolve to the same duplicate shadow spec (ignored below).
        unmatched_inputs.pop(input_dest, None)

        assert real_dest.startswith(subvol_prefix), (real_dest, subvol_prefix)
        container_dest = Path(real_dest[len(subvol_prefix) - 1 :])

        # Ignore duplicate `(container_dest, real_src)` pairs (redundancy is
        # OK), but error when the sources disagree (ambiguity is not).
        prev_src = container_dest_to_real_src.get(container_dest)
        if prev_src == real_src:
            continue
        set_new_key(container_dest_to_real_src, container_dest, Path(real_src))

        # Ban different `container_dest`s from being shadowed by the same
        # `real_src` because this can result in weird aliasing behavior
        # with updates via `librename_shadowed.so`.
        assert real_src not in real_srcs, f"{real_src} shadowed > 1 destination"
        real_srcs.add(real_src)

    # Check that every input `dest` was matched at least once.  Arguably, we
    # should not require filenames to match, since it's not guaranteed that
    # the file exists on `PATH`.  However, it's clearly an error for an
    # input absolute path not to exist.
    assert (
        not unmatched_inputs
    ), f"Shadow paths were not existing, regular files: {unmatched_inputs}"

    return container_dest_to_real_src


@contextmanager
def _copy_to_shadowed_root(subvol: Subvol, container_paths: Iterable[Path]):
    originals_and_backups = [
        (subvol.path(p), subvol.path(SHADOWED_PATHS_ROOT / p.lstrip(b"/")))
        for p in container_paths
    ]
    # This is redundant with our other "no ambiguity" and "no aliasing"
    # checks, so it should never be hit.
    assert 1 == len(
        {
            len(originals_and_backups),
            *(len(set(x)) for x in zip(*originals_and_backups)),
        }
    ), originals_and_backups
    try:
        # We don't use `--reflink=always` because in some debug-only
        # scenarios (see the description of the diff introducing this), it
        # makes sense to allow shadowing paths that come from mounts -- and
        # are thus both read-only, and possibly on a different FS.
        #
        # Falling back to `cp` incurs some I/O, but it should make no
        # difference in practice -- it's a debug-only fall-back.  In
        # principle, we could fall back to a bind mount instead, but the
        # implementation would be noticeably harder (especially the
        # `librename_shadowed.so` bits).
        #
        # Future: The directories we make under don't have the original
        # permissions.  I'm punting on fixing this, since our general thesis
        # is that build-time code is trusted.
        subvol.run_as_root(
            [
                "sh",
                "-uec",
                "\n".join(
                    textwrap.dedent(
                        f"""\
                b={backup.shell_quote()}
                b_dir=$(dirname "$b")
                mkdir -p "$b_dir"
                cp --reflink=auto --preserve=all {orig.shell_quote()} "$b"
                """
                    )
                    for orig, backup in originals_and_backups
                ),
            ]
        )
        yield
    finally:
        # As per the note above, the `cp` below will fail if we were
        # shadowing a mount -- all our mounts are currently read-only.  This
        # means that we can also get away with `--reflink=always`, we only
        # need `auto` above to support debug-only experiments.
        #
        # If you found a really good reason to improve support for this
        # situation, which only makes sense when you definitely never need
        # to update the shadowed file, there's low hanging-fruit.
        # Simply add `|| diff -q "$orig" "$backup"` -- there's no sense in
        # failing if the file hasn't changed.
        #
        # It IS possible to deal with failures to capture changes, too, but
        # I find this far-fetched, and so won't write out the solution here.
        #
        # Future: we could skip the "move back" part if we knew that the
        # intended use of the snapshot is ephemeral -- either because the
        # user explicitly told us via a flag, or because:
        #    opts.snapshot and not opts.debug_only_opts.snapshot_into
        # However, I think the savings in practice are too minimal to
        # bother with the extra complexity & requisite testing.
        subvol.run_as_root(
            [
                "sh",
                "-uec",
                "\n".join(
                    textwrap.dedent(
                        f"""\
                o={orig.shell_quote()}
                cp --reflink=always --preserve=all {backup.shell_quote()} "$o"
                rm {backup.shell_quote()}
                """
                    )
                    for orig, backup in originals_and_backups
                )
                + "\n"
                + textwrap.dedent(
                    f"""
                find {
                    subvol.path(SHADOWED_PATHS_ROOT).shell_quote()
                } -type d | sort -r | xargs rmdir
            """
                ),
            ]
        )


class ShadowPaths(NspawnPlugin):
    """
    `shadow_paths` has the form of {"/destination/to/shadow": "/with/what"},
    interpreted thus:
      - Source paths ("/with/what") are container-absolute.
      - If a destination path has a slash, it must be container-absolute.
      - Destination filenames are resolved to absolute paths via `PATH`.
      - If, after canonicalization, multiple inputs resolve to exactly the
        same (`destination`, `source`) pairs, those duplicates are ignored.
      - Any other duplicate `destination` or `source` entries are forbidden
        to avoid ambiguous behavior and aliasing.
    """

    def __init__(self, shadow_paths: Iterable[Tuple[Path, Path]]):
        self._shadow_paths = shadow_paths

    @contextmanager
    def wrap_setup(
        self,
        setup_ctx: _NspawnSetupCtxMgr,
        opts: _NspawnOpts,
        popen_args: PopenArgs,
    ) -> _NspawnSetup:
        container_dest_to_real_src = _resolve_to_canonical_shadow_paths(
            shadow_paths=self._shadow_paths,
            subvol=opts.layer,
            search_dirs=tuple(_shadow_search_dirs(opts.setenv)),
        )
        with setup_ctx(
            # The bind-mounts are only applied later, at popen time, so
            # they do not interfere with the copying we do below.
            opts._replace(
                bindmount_ro=(
                    *opts.bindmount_ro,
                    *((s, d) for d, s in container_dest_to_real_src.items()),
                )
            ),
            popen_args,
        ) as setup, _copy_to_shadowed_root(
            setup.subvol, container_dest_to_real_src.keys()
        ):
            yield setup
