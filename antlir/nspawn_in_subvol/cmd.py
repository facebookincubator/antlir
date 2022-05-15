#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
No externally useful functions here.  Read the `run.py` docblock instead.

Converts structures from `args.py` into a `systemd-nspawn` command-line.
"""
import base64
import os
import re
import subprocess
import uuid
from contextlib import contextmanager, nullcontext
from typing import AnyStr, Iterable, List, Mapping, NamedTuple, Optional, Tuple

from antlir.artifacts_dir import find_artifacts_dir
from antlir.compiler import procfs_serde
from antlir.compiler.items.common import META_ARTIFACTS_REQUIRE_REPO
from antlir.compiler.items.mount import mounts_from_meta
from antlir.config import repo_config
from antlir.find_built_subvol import find_built_subvol, Subvol
from antlir.fs_utils import Path, temp_dir
from antlir.send_fds_and_run import popen_and_inject_fds_after_sudo
from antlir.subvol_utils import TempSubvolumes
from antlir.unshare import Namespace, Unshare

from .args import _NspawnOpts, PopenArgs
from .common import find_cgroup2_mountpoint, parse_cgroup2_path


# For test mocking
_load_repo_config = repo_config


def _colon_quote_path(path: AnyStr) -> Path:
    return Path(re.sub(b"[\\\\:]", lambda m: b"\\" + m.group(0), Path(path)))


# NB: This assumes the path is readable to unprivileged users.
def _exists_in_image(subvol, path):
    return os.path.exists(subvol.path(path))


def bind_args(src, dest=None, *, readonly: bool = True):
    "dest is relative to the nspawn container root"
    if dest is None:
        dest = src
    # NB: The `systemd-nspawn` docs claim that we can add `:norbind` to make
    # the bind mount non-recursive.  This would be a bad default, so we
    # don't do it, but if you wanted to add it a non-recursive option, be
    # sure to test that nspawn actually implements the functionality -- it's
    # not very obvious from the code that it does (as of 8f6b442a7).
    return [
        "--bind-ro" if readonly else "--bind",
        f"{_colon_quote_path(src)}:{_colon_quote_path(dest)}",
    ]


def _inject_os_release_args(subvol):
    """
    nspawn requires os-release to be present as a "sanity check", but does
    not use it.  We do not want to block running commands on the image
    before it is created, so make a fake.
    """
    os_release_paths = ["/usr/lib/os-release", "/etc/os-release"]
    for path in os_release_paths:
        if _exists_in_image(subvol, path):
            return []
    # Not covering this with tests because it requires setting up a new test
    # image just for this case.  If we supported nested bind mounts, that
    # would be easy, but we do not.
    return bind_args("/dev/null", os_release_paths[0])  # pragma: no cover


@contextmanager
def _temp_cgroup(subvol: Subvol) -> Path:
    with open("/proc/self/cgroup", "rb") as cg_file:
        my_cg = parse_cgroup2_path(cg_file.read()).strip_leading_slashes()
    # This runs on the host, so we use the cgroup2 mountpoint we found
    new_cg = find_cgroup2_mountpoint() / Path(
        my_cg
        + b"/antlir-"
        # This is redundant with the UUID but aids debugging
        + str(os.getpid()).encode()
        + b"-"
        # Guaranteed unique on this host
        + base64.urlsafe_b64encode(uuid.uuid4().bytes).strip(b"=")
    )
    try:
        # pyre-fixme[7]: Expected `Path` but got `Generator[Path, None, None]`.
        yield new_cg
    finally:
        # Best-effort recursive cleanup of our cgroup.  This will only
        # succeed if neither it, nor its descendants contain any processes.
        #
        # Unfortunately, this cannot use `-print0` because our ambient `tac`
        # is too old to use `-s` to mean NUL-separated.  And this is not
        # important enough to write more code.
        #
        # I abuse `Subvol` as an easier-to-use "run under `sudo`" API.
        subvol.run_as_root(
            [
                "bash",
                "-uec",
                f"find {new_cg.shell_quote()} -type d | tac | xargs rmdir",
            ],
            # Leak without failing: checking this would likely result in
            # painful and fruitless investigations of flaky tests in CI
            # containers, which all get garbage-collected anyway.
            check=False,
        )


def _nspawn_cmd(
    nspawn_subvol: Subvol,
    temp_cgroup: Path,
    temp_bind_rootfs: Path,
    ns: Unshare,
):
    """
    This generates the exact command used to invoke systemd-nspawn for our
    runtime experience.
    - `nspawn_subvol`: This is the actual subvol that is used for the container.
        This is need to inspect and determine if a fake /etc/os-release file is
        required to satisfy systemd-nspawn requirements.
    - `temp_cgroup`:  A unique, temporary cgroup because as of 8/2020, upstream
        `systemd-nspawn --keep-unit` runs in a hardcoded `payload`
        sub-cgroup in the current scope. The result is that when
        we run multiple `nspawn_in_subvol` tests concurrently, they
        see each other's cgroups and interfere.
        Hopefully, we can fix this upstream, but that will take a while to
        sort out, so for now, we can manage this manually.
    - `temp_bind_rootfs`:  A temporary directory that is bind mounted to
        `nspawn_subvol`.  This is necessary to prevent a mount explosion
        when `--bind-repo-ro` is used.  In that case, the repository, which
        includes the actual btrfs volume where `nspawn_subvol` exists is
        exposed to the container.  This will end up causing a mount explosion
        because all of the mounts created by nspawn (/proc, /sys, /dev, etc..)
        will end up getting recursively included.  This is undesirable, so
        this path, which is outside the repository, is used to ensure that
        the runtime mounts don't get included.
    - ns: This is an unshare instance with just a unique mount namepsace that
        contains the bind mounts used for `temp_bind_rootfs`.
    """
    cmd = [
        "/bin/bash",
        "-uec",
        f"""
        new_cg={temp_cgroup.shell_quote()}
        mkdir "$new_cg"
        echo $$ > "$new_cg"/cgroup.procs
        exec "$@"
        """,
        "bash",  # $0 for the shell above
        # Without this, nspawn would look for the host systemd's cgroup setup,
        # which breaks us in continuous integration containers, which may not
        # have a `systemd` in the host container.
        #
        # We set this variable via `env` instead of relying on the `sudo`
        # configuration because it's important that it be set.
        "env",
        "UNIFIED_CGROUP_HIERARCHY=yes",
        *ns.nsenter_without_sudo(
            "systemd-nspawn",
            # Randomize --machine so that the container has a random hostname
            # each time. The goal is to help detect builds that somehow use the
            # hostname to influence the resulting image.
            "--machine",
            uuid.uuid4().hex,
            "--directory",
            str(temp_bind_rootfs),
            *_inject_os_release_args(nspawn_subvol),
            # Don't pollute the host's /var/log/journal
            "--link-journal=no",
            # Explicitly do not look for any settings for our ephemeral machine
            # on the host.
            "--settings=no",
            # The timezone should be set up explicitly, not by nspawn's fiat.
            "--timezone=off",  # requires v239+
            # Future: Uncomment.  This is good container hygiene.  It had to go
            # since it breaks XAR binaries, which rely on a setuid bootstrap.
            # '--no-new-privileges=1',
        ),
    ]

    return cmd


# This is a separate helper so that tests can mock it easily
def _artifacts_require_repo(src_subvol: Subvol) -> int:
    return procfs_serde.deserialize_int(
        src_subvol.path(), META_ARTIFACTS_REQUIRE_REPO.decode()
    )


def _extra_nspawn_args_and_env(
    opts: _NspawnOpts,
    # pyre-fixme[34]: `Variable[AnyStr <: [str, bytes]]` isn't present in the
    #  function's parameters.
) -> Tuple[
    List[AnyStr],  # Arguments to `systemd-nspawn`
    List[AnyStr],  # Environment variables to set when running `opts.cmd`
]:
    # NB: This does not set `--user` since this is done via `nsenter`
    extra_nspawn_args = []

    # Note: that nspawn_in_subvol only handles `BuildSource` mount
    # configurations.
    for mount in mounts_from_meta(opts.layer.path()):
        if mount.build_source.type == "host":
            extra_nspawn_args.extend(
                bind_args(
                    mount.build_source.source,
                    "/" + mount.mountpoint,
                    readonly=True,
                )
            )
        elif mount.build_source.type == "layer":
            target = mount.build_source.source
            extra_nspawn_args.extend(
                bind_args(
                    find_built_subvol(
                        opts.targets_and_outputs[str(target)]
                    ).path(),
                    "/" + mount.mountpoint,
                    readonly=True,
                )
            )

    if opts.quiet:
        # Otherwise stderr is polluted by useless messages like
        # 'Container 7fb953cb2c05457796e4b17351a12a36 exited successfully'
        extra_nspawn_args.append("--quiet")

    if opts.debug_only_opts.private_network:
        extra_nspawn_args.append("--private-network")

    if opts.bindmount_rw:
        for src, dest in opts.bindmount_rw:
            extra_nspawn_args.extend(bind_args(src, dest, readonly=False))

    if opts.bindmount_ro:
        for src, dest in opts.bindmount_ro:
            extra_nspawn_args.extend(bind_args(src, dest, readonly=True))

    if opts.bind_repo_ro or _artifacts_require_repo(opts.layer):
        # NB: Since this bind mount is only made within the nspawn
        # container, it is not visible in the `--snapshot-into` filesystem.
        # This is a worthwhile trade-off -- it is technically possible to
        # reimplement this kind of transient mount outside of the nspawn
        # container.  But, by making it available in the outer mount
        # namespace, its unmounting would become unreliable, and handling
        # that would add a bunch of complex code here.
        extra_nspawn_args.extend(
            bind_args(
                # Buck seems to operate with `realpath` when it resolves
                # `$(location)` macros, so this is what we should mount.
                os.path.realpath(_load_repo_config().repo_root)
            )
        )

        # insert additional host mounts that are always required when
        # using repository artifacts.
        for mount in _load_repo_config().host_mounts_for_repo_artifacts:
            extra_nspawn_args.extend(bind_args(mount))

        # Future: we **may** also need to mount the scratch directory
        # pointed to by `buck-image-out`, since otherwise repo code trying
        # to access other built layers won't work.  Not adding it now since
        # that seems like a rather esoteric requirement for the sorts of
        # code we should be running under `buck test` and `buck run`.  NB:
        # As of this writing, `mkscratch` works incorrectly under `nspawn`,
        # making `artifacts-dir` fail.

    # This has to be below the host_mounts_for_repo_artifacts binding to ensure
    # the artifacts dir bind isn't overwritten as readonly.
    # Future: Make a better API for interleaving rw and ro bindings.
    if opts.bind_artifacts_dir_rw:
        extra_nspawn_args.extend(
            bind_args(find_artifacts_dir().realpath(), readonly=False)
        )

    if opts.debug_only_opts.logs_tmpfs:
        extra_nspawn_args.extend(
            [
                "--tmpfs=/logs:"
                + ",".join(
                    [
                        f"uid={opts.user.pw_uid}",
                        f"gid={opts.user.pw_gid}",
                        "mode=0755",
                        "nodev",
                        "nosuid",
                        "noexec",
                    ]
                )
            ]
        )

    # Future: This is definitely not the way to go for providing device
    # nodes, but we need `/dev/fuse` right now to run XARs.  Let's invent a
    # systematic story later.  This cannot be a `feature` because of
    # the way that `nspawn` sets up `/dev`.
    #
    # Don't require coverage in case any weird test hosts lack FUSE.
    if os.path.exists("/dev/fuse"):  # pragma: no cover
        extra_nspawn_args.extend(["--bind=/dev/fuse"])

    if opts.debug_only_opts.cap_net_admin:
        extra_nspawn_args.append("--capability=CAP_NET_ADMIN")

    if opts.hostname:
        extra_nspawn_args.append(f"--hostname={opts.hostname}")

    # This is an internal option used only by TarballItem. The default is
    # false, meaning that mknod is not allowed.
    if not opts.allow_mknod:
        extra_nspawn_args.append("--drop-capability=CAP_MKNOD")

    if opts.debug_only_opts.register:
        extra_nspawn_args.append("--register=yes")
    else:
        extra_nspawn_args.append("--register=no")
        extra_nspawn_args.append("--keep-unit")

    cmd_env = []

    # Set the thrift env vars before we copy the user-supplied env vars,
    # so that if the if the user overides them, their version wins.
    if opts.debug_only_opts.forward_tls_env:
        for k, v in os.environ.items():
            if k.startswith("THRIFT_TLS_"):
                cmd_env.insert(0, f"{k}={v}")

    cmd_env.extend(opts.setenv)
    # This magic env var is also forwarded in `nspawn.py`.  We need it here
    # is since generators aren't touched by kernel cmdline `systemd.setenv`.
    if "ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP=1" in opts.setenv:
        extra_nspawn_args.append(
            "--setenv=ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP=1"
        )

    return extra_nspawn_args, cmd_env


@contextmanager
def _snapshot_subvol(
    src_subvol: Subvol, snapshot_into: Optional[AnyStr]
) -> Iterable[Subvol]:
    if snapshot_into:
        nspawn_subvol = Subvol(snapshot_into)
        nspawn_subvol.snapshot(src_subvol)
        yield nspawn_subvol
    else:
        with TempSubvolumes() as tmp_subvols:
            # To make it easier to debug where a temporary subvolume came
            # from, make make its name resemble that of its source.
            tmp_name = os.path.normpath(src_subvol.path())
            tmp_name = os.path.basename(
                os.path.dirname(tmp_name)
            ) or os.path.basename(tmp_name)
            nspawn_subvol = tmp_subvols.snapshot(src_subvol, tmp_name)
            yield nspawn_subvol


def nspawn_sanitize_env():
    env = os.environ.copy()
    # `systemd-nspawn` responds to a bunch of semi-private and intentionally
    # (mostly) undocumented environment variables.  Many of these can
    # compromise namespacing / isolation, which we emphatically do not want,
    # so let's prevent the ambient environment from changing them!
    #
    # Of course, this leaves alone a lot of the canonical variables
    # LINES/COLUMNS, or locale controls.  Those should be OK.
    for var in list(env.keys()):
        # No test coverage for this because (a) systemd does not pass such
        # environment vars to the container, so the only way to observe them
        # being set (or not) is via indirect side effects, (b) all the side
        # effects are annoying to test.
        if var.startswith("SYSTEMD_NSPAWN_"):  # pragma: no cover
            env.pop(var)
    return env


# NB: This could have been a function that returns a ctx manager, but that
# would create confusion since `popen`'s result **need not** be entered to
# be used, while that of `popen_and_inject_fds` must be.
@contextmanager
def maybe_popen_and_inject_fds(
    cmd: List[str], opts: _NspawnOpts, popen, *, set_listen_fds: bool
) -> Iterable[subprocess.Popen]:
    with (
        popen_and_inject_fds_after_sudo(
            cmd, opts.forward_fd, popen, set_listen_fds=set_listen_fds
        )
        if opts.forward_fd
        else popen(cmd)
    ) as proc:
        yield proc


class _NspawnSetup(NamedTuple):
    subvol: Subvol
    # pyre-fixme[34]: Current class isn't generic over `Variable[AnyStr <: [str,
    #  bytes]]`.
    nspawn_cmd: Iterable[AnyStr]  # How to invoke `systemd-nspawn`
    nspawn_env: Mapping[str, str]  # `{K: V}` env vars for `systemd-nspawn`
    opts: _NspawnOpts
    # pyre-fixme[34]: Current class isn't generic over `Variable[AnyStr <: [str,
    #  bytes]]`.
    cmd_env: Iterable[AnyStr]  # `K=V` env vars for `opts.cmd`
    popen_args: PopenArgs


@contextmanager
def _nspawn_subvol_setup(opts: _NspawnOpts) -> Subvol:
    with (
        # pyre-fixme[16]: `Iterable` has no attribute `__enter__`.
        _snapshot_subvol(opts.layer, opts.debug_only_opts.snapshot_into)
        if opts.snapshot
        else nullcontext(opts.layer)
    ) as nspawn_subvol:
        # pyre-fixme[7]: Expected `Subvol` but got `Generator[typing.Any, None,
        # None]`.
        yield nspawn_subvol


@contextmanager
def _nspawn_setup(
    nspawn_subvol: Subvol, opts: _NspawnOpts, popen_args: PopenArgs
) -> _NspawnSetup:
    # pyre-fixme[16]: `Path` has no attribute `__enter__`.
    with _temp_cgroup(opts.layer) as temp_cgroup, temp_dir(
        # Hardcoding /tmp is ugly, but buck will often set TMP to a path in
        # buck-out that ends up being underneath the repository and we need to
        # ensure that this bind location is separate from the repository dir.
        dir="/tmp",
        # Use a prefix that helps aids mere humans in debugging.
        prefix=f"antlir-{os.getpid()}-{nspawn_subvol.path().basename()}",
    ) as temp_bind_rootfs, Unshare([Namespace.MOUNT]) as ns:
        nspawn_args, cmd_env = _extra_nspawn_args_and_env(opts)
        nspawn_subvol.run_as_root(
            ns.nsenter_without_sudo(
                "mount",
                "--bind",
                str(nspawn_subvol.path()),
                str(temp_bind_rootfs),
            )
        )

        # pyre-fixme[7]: Expected `_NspawnSetup` but got `Generator[
        # _NspawnSetup, None, None]`.
        yield _NspawnSetup(
            subvol=nspawn_subvol,
            # pyre-fixme[60]: Concatenation not yet support for multiple variadic
            #  tuples: `*antlir.nspawn_in_subvol.cmd._nspawn_cmd(nspawn_subvol,
            #  temp_cgroup, temp_bind_rootfs, ns), *nspawn_args`.
            nspawn_cmd=(
                *_nspawn_cmd(nspawn_subvol, temp_cgroup, temp_bind_rootfs, ns),
                *nspawn_args,
            ),
            # This is a safeguard in case the `sudo` policy lets through any
            # unwanted environment variables.
            nspawn_env=nspawn_sanitize_env(),
            opts=opts,
            cmd_env=tuple(cmd_env),
            popen_args=popen_args,
        )
