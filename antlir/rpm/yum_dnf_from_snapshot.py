#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This tool wraps `yum` and its successor `dnf` to ensure more hermetic
behavior.

  - This code should only ever be executed in a no-network
    `nspawn_in_subvol` container, never on a bare host.

  - (Set up by `nspawn_in_subvol/plugins/repo_servers.py`): All RPM content
    is served by `repo_server.py` from an RPM repo snapshot captured by
    `snapshot_repos.py`, built via the `rpm_repo_snapshot()` Buck macro, and
    installed into some `image.layer` via the image feature named
    `install_rpm_repo_snapshot()`.

    Besides RPM repo data, the snapshot includes the `yum-dnf-from-snapshot`
    binary, a configuration file pointed at the appropriate repo-servers,
    and a warm cache for the package manager generated using the included
    repo snapshot.

    The intent is for both `antlir/` and the RPM snapshot to be committed
    to the source control repo, so that the source control repo revision
    hash completely determines the outcome of a package manager invocation.

  - `yum` or `dnf` run inside a mount namespace, with many of the files and
    directories that they might access on the host `image.layer` replaced by
    bind-mounts (the `--protected-path` option).

In other words, this provides additional sandboxing around RPM installation
in addition to the sandbox already provided by `nspawn_in_subvol`.

Sample usage:

    buck run TARGET_PATH:yum-dnf-from-snapshot -- --snapshot-dir REPOS_PATH \\
        dnf -- --installroot TARGET_DIR install --assumeyes some-package-name

Note that we have TWO `--` arguments, with the first protecting the
wrapper's arguments from `buck`, and the second protecting `yum` or `dnf`'s
arguments from the wrapper.

It should be safe to `--assumeyes` (which auto-imports GPG keys), because:
  - The snapshot repo server runs on localhost and only listens inside an
    ephemeral private network namespace, making compromise unlikely.
  - The repo server verifies RPM & repodata checksums against the
    version-controlled snapshot before sending them out.
  - Snapshotted repos are required to have `gpgcheck` enabled. When
    `snapshot-repos` downloads GPG keys, it checks them against a
    predetermined allowlist, protecting us against transient key injections.
    Many other sanity checks happen at snapshot time.

This binary normally runs inside a build appliance (see `RpmActionItem`).
The code here thus uses the BA's `yum` or `dnf` binary, so build appliance
upgrades can break this code.

## Future work

The current tool works well, with these caveats:

  - `yum` & `dnf` leave various caches inside `--installroot`, which bloat
    the image.  `RpmActionItem` has a bind-mount to prevent this leakage,
    and we also provide `//antlir/features:rpm_cleanup` which should be
    included in all production layers.

  - Base CentOS packages deposit a vanilla CentOS `yum` configuration into
    the install root via `/etc/yum.repos.d/`, bearing no relation to the
    `yum.conf` that was used to install packages into the image.  Note that
    `dnf` would try to look in the same `yum.repos.d` if we did not hide it.

  - `nspawn_in_subvol` brings up and tears down network namespaces
    frequently.  According to ast@kernel.org, bugs are routinely introduced
    that break NETNS clean-up, which may cause us to leak namespaces in
    production.  If this becomes an issue, we can try cgroup-bpf style
    firewalling instead, along the lines of the program in `bind4_prog_load`
    in the kernel's `test_sock_addr.c`.

  - When installing into a blank root, `yum/dnf` cannot discover the
    release, so it literally has `/repos/x86_64/$releasever` as the
    'persistdir' subdirectory.  How should we determine the correct release
    for a snapshot-based install?  Fake it?  Add `/etc/*-release` from the
    snapshot host to the snapshot?
"""
import argparse
import base64
import logging
import os
import pwd
import shutil
import subprocess
import tempfile
import textwrap
import uuid
from configparser import ConfigParser
from contextlib import contextmanager, nullcontext
from typing import Iterable, List, Mapping, Optional

from antlir.common import get_logger
from antlir.fs_utils import META_DIR, Path, temp_dir
from antlir.nspawn_in_subvol.plugins.shadow_paths import SHADOWED_PATHS_ROOT

from antlir.rpm.common import has_yum, yum_is_dnf
from antlir.rpm.yum_dnf_conf import YumDnf


log = get_logger()

# We expect this to be provided as part of the layer that's executing `yum`
# (most frequently, this is the build appliance).  The reason is that to be
# perfectly correct, this `LD_PRELOAD` library has to be built with the same
# toolchain as the `glibc` that we're interposing.
#
# It's not under `/__antlir__/rpm/` because it's not actually RPM-specific.
#
# Note that this won't work out of the box to allow updating shadowed paths
# that are under `--installroot` -- to fix it, we would need to make a
# "remapped" shadow root be available inside the install root, so that it
# can be seen by the `chroot`ed `rename` function call.  This is obviously
# not worth the trouble in the absence of a VERY compelling need.
LIBRENAME_SHADOWED_PATH = Path("/__antlir__/librename_shadowed.so")

# This wrapper won't work outside of / if this is violated.
assert SHADOWED_PATHS_ROOT.startswith(b"/"), SHADOWED_PATHS_ROOT

# This is yucky, but `test_update_shadowed` must not mock ALL uses of
# `SHADOWED_PATHS_ROOT`, or it will be unable to find the original RPM
# installer binary.  So we make this mock point available.
_LIBRENAME_SHADOWED_PATHS_ROOT = SHADOWED_PATHS_ROOT


# To distinguish the main process from other `CalledProcessError`s
class _YumDnfError(subprocess.CalledProcessError):
    def __repr__(self):
        # Compact repr for `=container` target interactive use.
        return f"YumDnfError(returncode={self.returncode})"


def _install_to_current_root(install_root):
    return install_root.realpath() == b"/"


# Yum is incorrigible -- it is impossible to give it a set of options
# that will completely prevent it from accessing host configuration &
# caches.  So instead, we do this to avoid littering inside the
# surrounding `nspawn_in_subvol` container:
#
#  - `YumDnfConfIsolator` coerces the config to "isolated" or "default",
#    as much as possible.
#
#  - In our mount namespace, we bind-mount no-op files and directories
#    on top of all the configuration paths that `yum` might try to
#    access on the host (whether it does or not).  The sources for this
#    information are (i) `man yum.conf`, (ii) `man rpm`, and (iii)
#    adding `strace -ff -e trace=file -oyumtrace` below.  To check the
#    isolation, one may grep for "/(etc|var)" in the traces, keeping in
#    mind that many of the accesses happen in chroots.  E.g.
#
#      grep '(".*"' yumtrace.* | cut -f 2 -d\\" |
#        grep -v '/tmp/tmp[^/]*install/' | sort -u | less -N
#
#    Note that once `yum` starts chrooting into the install root, one
#    has to do a bit of work to filter out the chrooted actions.  It's
#    not too painful to cut out the bulk of them so manually with an
#    editor, after verifying thus that all the chrooted accesses come in
#    one continuous block:
#
#      grep -v '^[abd-z][a-z]*("/\\+tmp/tmp9q0y0pshinstall' \
#        yumtrace.3803399 |
#        egrep -v '"(/proc/self/loginuid|/var/tmp|/sys/)' |
#        egrep -v '"(/etc/selinux|/etc/localtime|/")' |
#        python3 -c 'import sys;print(sys.stdin.read(
#        ).replace(
#        "chroot(\\".\\")                             = 0\\n" +
#        "chroot(\\"/tmp/tmp9q0y0pshinstall/\\")      = 0\\n",
#        ""
#        ))'| less -N
#
#    Even though that still leaves some child processes that ran
#    entirely inside a chroot, the post-edit file list was still
#    possible to vet by hand, since the bulk of accesses fell into
#    /lib*, /opt, /sbin, and /usr.
#
#    NB: I kept access to:
#     /usr/lib/rpm/rpmrc /usr/lib/rpm/redhat/rpmrc
#     /usr/lib/rpm/macros /usr/lib/rpm/redhat/macros
#   on the premise that unlike the local customizations, these may be
#   required for `rpm` to function.
def _isolate_yum_dnf(
    yum_dnf: YumDnf,
    install_root,
    *,
    dummy_dev: Path,
    protected_path_to_dummy,
    cache_dir: Path,
):
    "Isolate yum/dnf from the host filesystem."
    # When installing to a chroot, provide a mock `/dev/null` so that
    # post-install scripts can write to `/dev/null`.  If we used nspawn to
    # sandbox the install, this would be taken care of, but it would also
    # litter the install root, so we haven't done it yet. A hack for now:
    #
    # Note that our mock `/dev` must be read-write in case a package like
    # `filesystem` is installed and wants to mutate `/dev/`.  Such changes
    # will be gleefully discarded.
    set_up_dev = (
        """\
install_root={quoted_install_root}
mkdir -p "$install_root"/dev/
chown root:root "$install_root"/dev/
chmod 0755 "$install_root"/dev/
mount {quoted_dummy_dev} "$install_root"/dev/ -o bind
mount /dev/null "$install_root"/dev/null -o bind
""".format(
            quoted_dummy_dev=dummy_dev.shell_quote(),
            quoted_install_root=install_root.shell_quote(),
        )
        if not _install_to_current_root(install_root)
        else ""
    )
    return [
        "bash",
        *(["-x"] if log.isEnabledFor(logging.DEBUG) else []),
        "-o",
        "pipefail",
        "-uec",
        textwrap.dedent(
            """\
{quoted_maybe_set_up_dev}
{quoted_maybe_clone_cache_dir}
mkdir -p -m 0755 /var/cache/{prog_name}  # must exist to be protected
{quoted_protected_paths}

# `yum` & `dnf` also use the host's /var/tmp, and since I don't trust
# them to isolate themselves, let's also relocate that.
var_tmp=$(mktemp -d --suffix=_isolated_{prog_name}_var_tmp)
mount "$var_tmp" /var/tmp -o bind

# Clean up the isolation directories. Since we're running as `root`,
# `rmdir` feels a lot safer, and also asserts that we did not litter.
trap 'rmdir "$var_tmp"' EXIT

# NB: The `trap` above means the `bash` process is not replaced by the
# child, but that's not a problem.
{maybe_set_env_vars} exec "$@"
"""
        ).format(
            prog_name=yum_dnf.value,
            quoted_maybe_set_up_dev=set_up_dev,
            # Read `_set_up_yum_dnf_cache` for the rationale.
            quoted_maybe_clone_cache_dir=(
                "cache_dir="
                if not cache_dir or _install_to_current_root(install_root)
                else f"""
                cache_dir={cache_dir.shell_quote()}
                mount -o bind "$cache_dir" "$install_root/$cache_dir"
                """
            ),
            quoted_protected_paths="\n".join(
                "mount {} {} -o bind,ro".format(
                    dummy.shell_quote(),
                    (
                        # Convention: relative for image, or absolute for host.
                        ""
                        if prot_path.startswith(b"/")
                        else '"$install_root"/'
                    )
                    + prot_path.shell_quote(),
                )
                for prot_path, dummy in protected_path_to_dummy.items()
            ),
            maybe_set_env_vars=" ".join(
                [
                    f"LD_PRELOAD={LIBRENAME_SHADOWED_PATH.shell_quote()}",
                    (
                        "ANTLIR_SHADOWED_PATHS_ROOT="
                        f"{_LIBRENAME_SHADOWED_PATHS_ROOT.shell_quote()}"
                    ),
                ]
            )
            if os.path.exists(LIBRENAME_SHADOWED_PATH)
            else "",
        ),
    ]


@contextmanager
def _dummy_dev() -> Path:
    "An allowlist of devices is safer than the entire host /dev"
    dummy_dev = Path(tempfile.mkdtemp())
    try:
        subprocess.check_call(["sudo", "chown", "root:root", dummy_dev])
        subprocess.check_call(["sudo", "chmod", "0755", dummy_dev])
        subprocess.check_call(["sudo", "touch", dummy_dev / "null"])
        # pyre-fixme[7]: Expected `Path` but got `Generator[Path, None, None]`.
        yield dummy_dev
    finally:
        # We cannot use `temp_dir` for cleanup since the directory and
        # contents are owned by root.  Remove recursively since RPMs like
        # `filesystem` can touch this dummy directory.  We will discard
        # their writes, which do not, anyhow, belong in a container image.
        subprocess.run(["sudo", "rm", "-r", dummy_dev])


@contextmanager
def _dummies_for_protected_paths(
    install_root: Path,
    must_exist: Iterable[str],
    may_exist: Iterable[str],
) -> Mapping[Path, Path]:
    """
    Some locations (some host yum/dnf directories, and install root /.meta/
    and mountpoints) should be off-limits to writes by RPMs.  We enforce
    that by bind-mounting an empty file or directory on top of each one.
    """
    protected_paths = [*must_exist]
    for p in may_exist:
        # Convention: relative for image, or absolute for host.
        path = Path(p) if p.startswith("/") else (install_root / p)
        # Don't protect symlinks
        if path.exists(raise_permission_error=True) and not path.islink():
            protected_paths.append(p)
    with temp_dir() as td, tempfile.NamedTemporaryFile() as tf:
        # NB: There may be duplicates in protected_paths, so we normalize.
        # If the duplicates include both a file and a directory, this picks
        # one arbitrarily, and if the type on disk is different, we will
        # fail at mount time.  This doesn't seem worth an explicit check.
        # pyre-fixme[7]: Expected `Mapping[Path, Path]` but got
        #  `Generator[typing.Dict[Path, Path], None, None]`.
        yield {
            Path(p).normpath(): (td if p.endswith("/") else Path(tf.name))
            for p in protected_paths
        }
        # NB: The bind mount is read-only, so this is just paranoia.  If it
        # were left RW, we'd need to check its owner / permissions too.
        for expected, actual in (([], td.listdir()), (b"", tf.read())):
            assert expected == actual, f"Some RPM wrote {actual} to {protected_paths}"


def _ensure_antlir_container():
    """
    Forbid running this outside of an `antlir/nspawn_in_subvol` container.
    Since we default to `--installroot=/`, there is some risk to allowing
    execution in other settings.
    """
    # Any `antlir` container with snapshots must have `/__antlir__`
    assert os.path.isdir(
        "/__antlir__"
    ), "`yum-dnf-from-snapshot` must run in an `nspawn_in_subvol` container"
    # Future: are there other checks we can add?


def _ensure_private_network():
    """
    Normally, we run under `systemd-nspawn --private-network`.  We don't
    want to run in environments with network access because in these cases
    it's very possible that `yum` / `dnf` will end up doing something
    non-deterministic by reaching out to the network.
    """
    # From `/usr/include/linux/if_arp.h`
    allowed_types = {
        1,  # ARPHRD_ETHER
        768,  # ARPHRD_TUNNEL
        769,  # ARPHRD_TUNNEL6
        772,  # ARPHRD_LOOPBACK
        778,  # ARPHRD_IPGRE
        823,  # ARPHRD_IP6GRE
    }
    net = Path("/sys/class/net")
    for iface in net.listdir():
        # Not *every* directory in /sys/class/net is
        # a symlink to an interface device directory
        # A specific case is /sys/class/net/bonding_masters
        # which is present if the bonding module is loaded
        iface_dir = net / iface
        iface_type = Path(iface_dir / "type")
        if os.path.isdir(iface_dir) and iface_type.exists():
            iface_type = int(iface_type.read_text())
            # Not covered because we don't want to rely on the CI container
            # having a network interface.
            if iface_type not in allowed_types:  # pragma: no cover
                raise RuntimeError(
                    "Refusing to run without --private-network, found "
                    f"unknown interface {iface} of type {iface_type}."
                )


def _install_root(conf_path: Path, yum_dnf_args: Iterable[str]) -> Path:
    # Peek at the `yum` / `dnf` args, which take precedence over the config.
    p = argparse.ArgumentParser(allow_abbrev=False, add_help=False)
    p.add_argument("--installroot", type=Path.from_argparse)
    # pyre-fixme[6]: Expected `Optional[typing.Sequence[str]]` for 1st param but
    #  got `Iterable[str]`.
    args, _ = p.parse_known_args(yum_dnf_args)
    if args.installroot:
        return args.installroot
    # For our wrapper to be transparent, the `installroot` semantics have to
    # match that of `yum` / `dnf`, so the argument is optional, with a
    # fallback to the config file, and then to `/`.
    cp = ConfigParser()
    with open(conf_path) as conf_in:
        cp.read_file(conf_in)
    return Path(cp["main"].get("installroot", "/"))


def _resolve_rpm_installer_binary(
    yum_dnf: YumDnf, yum_dnf_binary: Optional[Path]
) -> Path:
    """
    Returns an absolute path to the "original" RPM installer binary.  It
    will typically be in `/usr/` or in the corresponding "shadow root" path.
    """
    if yum_dnf_binary is None:
        yum_dnf_binary = Path(shutil.which(yum_dnf.value))
    assert yum_dnf_binary.startswith(b"/"), yum_dnf_binary
    # We must canonicalize here because the shadowing code does so (to avoid
    # duplicate shadows due to aliasing etc).
    yum_dnf_binary = Path(yum_dnf_binary).realpath()
    # If it becomes a problem to invoke the RPM installer out of the shadow
    # root (i.e. if it cares about its prefix), we can fix this by making
    # `yum_dnf_from_snapshot.py` unmount the shadow bind mount in its
    # private mount NS.  Caveat: we'd also need a "protective" RO bind mount
    # on top, because otherwise an `unlink` in the mount NS would remove the
    # bind mount in the parent NS.
    shadowed_binary = SHADOWED_PATHS_ROOT / yum_dnf_binary.strip_leading_slashes()

    # We can't cover both branches in the same `image_python_unittest`,
    # since either the binary will or won't be shadowed.  However, one can
    # manually verify that each variant of `test-yum-dnf-from-snapshot-*`
    # will cover a different side of the branch.

    # This is also covered by `test_rpm_installer_shadow_paths.py`.
    if os.path.exists(shadowed_binary):  # pragma: no cover
        log.debug(f"Using shadowed installer {shadowed_binary}")
        return shadowed_binary
    else:  # pragma: no cover
        log.debug(f"no {shadowed_binary}, using unshadowed installer {yum_dnf_binary}")
        return yum_dnf_binary


@contextmanager
def _set_up_yum_dnf_cache(
    yum_dnf: YumDnf, install_root: Path, snapshot_dir: Path
) -> Path:
    """
    Reflink-copy (and clean up on exit) the snapshot's repodata cache into
    `install_root / <random string>`.  Yield the `install_root`-relative
    path.

    We don't want to use `cachedir=/__antlir__/rpm/...` because the RPM
    installers want to put the cache **inside** `install_root`.  If we did
    use the original `__antlir__` path, we would need to clean up parts of a
    nested directory tree that could be shared with real artifacts from the
    image under construction.  That is much more messy and error-prone than
    having a dedicated path.

    We copy instead of bind-mounting because it's possible that a user will
    run concurrent installs from the same snapshot.  With a bind-mounted
    cache, the RPM installer would either hit contention (likely -- it has
    locking) or corruption (not impossible -- this locking is not tested in
    a bind-mount setup).

    On the other hand, a copy is very cheap thanks to `btrfs`, and
    eliminates concurrency bugs.
    """
    # Our ephemeral cache set-up is very particular, for reasons that boil
    # down to the fact that as of 2020, Linux does not allow reflinks to
    # cross mounts, even if they are on the same filesystem.  See e.g. the
    # discussion here:
    #     https://lore.kernel.org/linux-fsdevel/CAOQ4uxj1csY-
    #         Vn2suFZMseEZgvAZzhQ82TR+XtDRQ=cOzwvzzw@mail.gmail.com/
    #
    # Here are the actors in our play (assuming `install_root != "/"`):
    #   - snapshot cache:
    #     `/__antlir__/rpm/repo-snapshot/*//{prog}/var/cache/{prog}`
    #   - ephemeral copy of the snapshot cache: `/cache_name`
    #   - bind-mount of the ephemeral copy: `install_root/cache_name`
    #
    # First, we never want to try to do a reflink copy from the snapshot
    # cache to the install root, because that can be a bind mount (e.g. in
    # `RpmActionItem`).  This is why we copy to `/cache_name`, and then
    # bind-mount into the install root.
    #
    # Second, genrule layers do `mount -o bind /__antlir__ /__antlir__` to
    # prevent accidental changes to `/__antlir__`.  However, this breaks
    # reflink copies from the snapshot cache (in its own bind-mount) to the
    # container root.
    #
    # To fix the second issue, we run our reflink copy in a mount namespace,
    # and (transiently) re-purpose `/cache_name` to bind-mount `/`. This has
    # the effect of stripping away the `__antlir__` bind-mount, and letting
    # us do the reflink on the root FS.
    #
    # IMPORTANT: This means that `/__antlir__` must always be on the root
    # filesystem, it cannot e.g. by an `feature.layer_mount`.  This is a
    # reasonable restriction, because the cache contents is coupled to the
    # content of the root FS in any case -- the `yum` / `dnf` version must
    # match for the cache to make sense.
    #
    # The above messy setup also works around the bug in `dnf` PR 1672,
    # because we end up with a copy of the cache both in the container and
    # in the install root, at the same location.
    cache_name = base64.urlsafe_b64encode(uuid.uuid4().bytes).strip(b"=")
    cache_dest = Path(b"/" + cache_name)
    prog = yum_dnf.value
    install_to_cur_root = _install_to_current_root(install_root)
    try:
        os.mkdir(cache_dest)  # needed for the `/` bind-mount below
        if not install_to_cur_root:
            os.mkdir(install_root / cache_name)  # `_isolate_yum_dnf` bind-mount
        log.debug(f"Setting up ephemeral {prog} cache in {cache_dest}")
        subprocess.check_call(
            [
                "sudo",
                "unshare",
                "-m",
                "bash",
                "-uec",
                ";".join(
                    [
                        # This odd `mount` is explained in the long doc above.
                        f"mount -o bind / {cache_dest}",
                        " ".join(
                            [
                                "cp",
                                "--archive",
                                "--reflink=always",
                                "--no-target-directory",
                                (
                                    cache_dest
                                    / snapshot_dir.strip_leading_slashes()
                                    / f"{prog}/var/cache/{prog}"
                                ).shell_quote(),
                                (cache_dest / cache_name).shell_quote(),
                            ]
                        ),
                    ]
                ),
            ]
        )
        # pyre-fixme[7]: Expected `Path` but got `Generator[Path, None, None]`.
        yield cache_dest
    finally:
        if not install_to_cur_root:
            os.rmdir(install_root / cache_name)
        # The assert is paranoia to make sure we don't `rm` something wrong.
        assert cache_dest == b"/" + cache_name, cache_dest
        subprocess.check_call(["sudo", "rm", "-rf", cache_dest])


def yum_dnf_from_snapshot(
    *,
    yum_dnf: YumDnf,
    snapshot_dir: Path,
    protected_paths: List[str],
    yum_dnf_args: List[str],
    yum_dnf_binary: Optional[Path] = None,
):
    yum_dnf_binary = _resolve_rpm_installer_binary(yum_dnf, yum_dnf_binary)
    _ensure_antlir_container()
    _ensure_private_network()

    prog_name = yum_dnf.value
    # This path convention must match how `write_yum_dnf_conf.py` and
    # `rpm_repo_snapshot.bzl` set up their output.
    conf_path = snapshot_dir / f"{prog_name}/etc/{prog_name}/{prog_name}.conf"
    install_root = _install_root(conf_path, yum_dnf_args)

    # The protected path logic below and in `RpmActionItem` assumes this:
    assert not META_DIR.startswith(b"/")
    # The paths that have trailing slashes are directories, others are
    # files. If you omit the leading slash, this path is relative to
    # `installroot`.
    protected_paths = [  # do not mutate the function argument
        *protected_paths,
        # Protect the cache even when installing to / because the
        # snapshot has its own cache.
        f"/var/cache/{prog_name}/",
        # RPM must never touch META_DIR on the host filesystem.  We are sure
        # that it exists since snapshots only exist in Antlir-built images.
        f"/{META_DIR}",
    ]
    optional_protected_paths = []
    if not _install_to_current_root(install_root):
        # Ensure the host log exists, so we can guarantee we don't write to it.
        log_path = f"/var/log/{prog_name}.log"
        subprocess.check_call(["sudo", "touch", log_path])
        # If we are installing to /, it makes no sense to isolate /etc, or the
        # RPM installer DBs, or the install logs -- `rpm` can write there.
        protected_paths.extend(
            [
                # See the `_isolate_yum_dnf` docblock for how (and why) this
                # list was produced.  All are assumed to exist on the host
                # -- otherwise, we'd be in the awkard situation of leaving
                # them unprotected, or creating them on the host.
                "/etc/yum.repos.d/",  # dnf ALSO needs this isolated
                f"/etc/{prog_name}/",  # Also covers /etc/dnf/dnf.conf
                "/etc/pki/rpm-gpg/",
                "/etc/rpm/",  # Also covers /etc/rpm/macros
                log_path,
                f"/var/lib/{prog_name}/",
            ]
            + (
                # Fedora's `yum` is a symlink to `dnf`, so `/etc/yum` is absent
                # When `yum_dnf == yum`, this duplicates `/etc/{prog_name}`.
                ["/etc/yum/"]
                if (has_yum() and not yum_is_dnf())
                else []
            )
        )
        # Centos9 is moving the rpm db path from /var/lib/rpm/ to
        # /usr/lib/sysimage/rpm, so protect whichever one exists.
        optional_protected_paths.extend(
            [
                "/usr/lib/sysimage/rpm/",
                "/var/lib/rpm/",
            ]
        )
        # Also protect potentially non-hermetic files that are not required
        # to exist on the host.  We don't expect these to be written, only
        # read, so failing to protect the non-existent ones is OK.
        user_home = pwd.getpwnam("root").pw_dir  # Assume we `sudo` as `root`.
        optional_protected_paths.extend(
            [
                user_home + "/.rpmrc",
                "/etc/rpmrc",
                user_home + "/.rpmmacros",
            ]
        )
        # Unlike `/etc/dnf/dnf.conf` this isn't protected by an outer directory
        if yum_dnf == YumDnf.yum:  # pragma: no cover
            optional_protected_paths.append("/etc/yum.conf")
        # Protect `install_root / META_DIR` if it exists, because it should
        # always be off-limits to RPMs -- it is for `antlir/compiler/`
        # alone.  NB: `RpmActionItem` will also add it to `protected_paths`.
        #
        # But, don't **require** META_DIR to be present, to permit the
        # following normal usage of Antlir containers:
        #    buck run :ba=container -- --user=root
        #    mkdir /i1
        #    dnf install -y --installroot=/i1 jq
        optional_protected_paths.append(META_DIR.decode())

    for arg in yum_dnf_args:
        assert arg != "-c" and not arg.startswith(
            "--config"
        ), "If you change --config, you will no longer use the repo snapshot"

    # NB: The subsequnt verb tests intentionally under-match because options
    # could precede the command verb.  However, this is guaranteed not to
    # overmatch, and it will definitely work with the Antlir code that
    # depends on this behavior, so it's good enough.
    #
    # For `image_yum_dnf_make_snapshot_cache` only.
    is_makecache = yum_dnf_args[:1] == ["makecache"]

    # pyre-fixme[16]: `Path` has no attribute `__enter__`.
    # pyre-fixme[16]: `Mapping` has no attribute `__enter__`.
    with _dummy_dev() as dummy_dev, _dummies_for_protected_paths(
        install_root=install_root,
        must_exist=protected_paths,
        may_exist=optional_protected_paths,
    ) as protected_path_to_dummy, (
        # pyre-fixme[16]: Item `Path` of `Union[nullcontext[None], Path]` has no
        #  attribute `__enter__`.
        nullcontext()
        if is_makecache
        else _set_up_yum_dnf_cache(yum_dnf, install_root, snapshot_dir)
    ) as cache_dir:
        cmd = [
            "sudo",
            # We need `--mount` so as not to leak our `--protect-path`
            # bind mounts outside of the package manager invocation.
            #
            # Note that `--mount` implies `mount --make-rprivate /` for
            # all recent `util-linux` releases (since 2.27 circa 2015).
            #
            # We omit `--net` because `yum-dnf-from-snapshot` should
            # only be running in a private-network `nspawn_in_subvol` at
            # this point, and `repo_servers.py` servers listen on
            # sockets that are outside of this `unshare` (the latter
            # could be changed but requires laboriously punching through
            # some abstraction boundaries).
            #
            # `--uts` and `--ipc` are set just because they're free to
            # namespace.  We couldn't do `--pid` or `--cgroup` without
            # significant extra work, which has no clear value (i.e.
            # we'd effectively need to use `systemd-nspawn` here).
            "unshare",
            "--mount",
            "--uts",
            "--ipc",
            *_isolate_yum_dnf(
                yum_dnf,
                install_root,
                dummy_dev=dummy_dev,
                protected_path_to_dummy=protected_path_to_dummy,
                cache_dir=cache_dir,
            ),
            "yum-dnf-from-snapshot",  # argv[0]
            yum_dnf_binary,
            # Only permit known-good / known-needed plugins out of paranoia.
            # Since we always run under a no-network build appliance, it's
            # hard to think of a plugin that might do something truly
            # atrocious, so it may be reasonable to relax this later.
            "--disableplugin=*",
            # `versionlock` is used by Antlir's version selection.
            # `download` is nice so that folks can easily get snapshot RPMs:
            # `flunk_dependent_remove` will fail dnf remove operations that
            # would otherwise silently remove dependent rpms.
            #    buck run :x=container -- --user=root -- dnf download ...
            "--enableplugin=versionlock,download,flunk_dependent_remove,builddep",
            # Config options get isolated by our `YumDnfConfIsolator`
            # when `write-yum-dnf-conf` builds this file.  Note that
            # `yum` doesn't work if the config path is relative.
            f"--config={conf_path.abspath()}",
            # Expose the snapshot's cache as an ephemeral copy.
            #
            # NB: Prior to PR 1672, `dnf` would ignore the `install_root`
            # cache and, worse, dump the cache to the ambient OS.
            # `_isolate_yum_dnf` provides a `cache_dir` workaround.
            *([f"--setopt=cachedir={cache_dir}"] if cache_dir else []),
            f"--installroot={install_root}",
            # NB: We omit `--downloaddir` because the default behavior
            # is to put any downloaded RPMs in `$installroot/$cachedir`,
            # which is reasonable, and easy to clean up in a post-pass.
            *yum_dnf_args,
        ]
        try:
            subprocess.check_call(cmd)
        except subprocess.CalledProcessError as ex:
            # In interactive use (`=container` targets), it is very jarring
            # to see the entire internal commandline.  This hides it.
            raise _YumDnfError(**ex.__dict__)


def main() -> None:  # pragma: no cover
    import shlex
    import sys

    from antlir.cli import init_cli

    with init_cli(__doc__) as cli:
        cli.parser.add_argument(
            "--snapshot-dir",
            required=True,
            type=Path.from_argparse,
            help="Multi-repo snapshot directory.",
        )
        # When a wrapper from an RPM repo snapshot is shadowing an OS rpm
        # installer, it needs this argument to invoke the exact binary that
        # it is shadowing.
        #
        # Caveat: the basename of this path may differ from the `yum_dnf`
        # argument below because on Fedora, `yum` is a symlink to `dnf`.
        cli.parser.add_argument(
            "--yum-dnf-binary",
            type=Path.from_argparse,
            help="Optional absolute path, defaults to resolving the `yum_dnf` "
            "argument via `PATH`. This is the non-shadowed path to the "
            "actual RPM installer binary that we are wrapping.",
        )
        cli.parser.add_argument(
            "--protected-path",
            action="append",
            default=[],
            # Future: if desired, the trailing / convention could be
            # relaxed, see `_protected_path_set`.  If so, this program would
            # just need to run `os.path.isdir` against each of the paths.
            help="When `yum` or `dnf` runs, this path will have an empty file "
            "or directory read-only bind-mounted on top. If the path has a "
            "trailing /, it is a directory, otherwise -- a file. If the path "
            "is absolute, it is a host path. Otherwise, it is relative to "
            "`--installroot`. The path must already exist. There are some "
            "internal defaults that cannot be un-protected. May be repeated.",
        )
        cli.parser.add_argument("yum_dnf", type=YumDnf, help="yum or dnf")
        cli.parser.add_argument(
            "args",
            nargs="+",
            help="Pass these through to `yum` or `dnf`. You will want to use "
            "-- before any such argument to prevent `yum-dnf-from-snapshot` "
            "from parsing them. Avoid arguments that might break hermeticity "
            "(e.g. affecting the host system, or making us depend on the "
            "host system) -- this tool implements protections, but it "
            "may not be foolproof.",
        )

    args = cli.args
    try:
        yum_dnf_from_snapshot(
            yum_dnf=args.yum_dnf,
            yum_dnf_binary=args.yum_dnf_binary,
            snapshot_dir=args.snapshot_dir,
            protected_paths=args.protected_path,
            yum_dnf_args=args.args,
        )
    except BaseException as ex:
        what_ran = f"""`{args.yum_dnf.value} {
            ' '.join(shlex.quote(a) for a in args.args)
        }` from snapshot `{args.snapshot_dir}`"""
        if args.debug:
            log.exception(f"While running {what_ran}:")
        else:
            # Dumping a long stack trace obscures the actual yum/dnf error.
            log.exception(
                f"""{repr(ex)} while running {what_ran}. """
                f"For more logs, run your container and `{args.yum_dnf.value}` "
                "command with `ANTLIR_DEBUG=1`."
            )
        sys.exit(
            ex.returncode
            if isinstance(ex, subprocess.CalledProcessError)
            else 1  # No return code to forward
        )


# This argument-parsing logic is covered by RpmActionItem tests.
if __name__ == "__main__":
    main()  # pragma: no cover
