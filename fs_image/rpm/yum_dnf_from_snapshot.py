#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
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
    and (in the very near future) a warm cache for the package manager
    generated using the included repo snapshot.

    The intent is for both `fs_image/` and the RPM snapshot to be committed
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
    predetermined whitelist, protecting us against transient key injections.
    Many other sanity checks happen at snapshot time.

This binary normally runs inside a build appliance (see `RpmActionItem`).
The code here thus uses the BA's `yum` or `dnf` binary, so build appliance
upgrades can break this code.

## Future work

The current tool works well, with these caveats:

  - `yum` & `dnf` leave various caches inside `--installroot`, which bloat
    the image.  `RpmActionItem` has a bind-mount to prevent this leakage,
    and we also provide `//fs_image/features:rpm_cleanup` which should be
    included in all production layers. Future: once caches are part of the
    snapshot, this issue can be fixed completely.

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
'''
import argparse
import os
import subprocess
import tempfile
import textwrap

from configparser import ConfigParser
from contextlib import contextmanager
from typing import Iterable, List, Mapping

from fs_image.common import (
    check_popen_returncode, get_file_logger, init_logging,
)
from fs_image.fs_utils import Path, temp_dir
from fs_image.nspawn_in_subvol.plugins.shadow_paths import SHADOWED_PATHS_ROOT

from .yum_dnf_conf import YumDnf
from .common import has_yum, yum_is_dnf

log = get_file_logger(__file__)

# We expect this to be provided as part of the layer that's executing `yum`
# (most frequently, this is the build appliance).  The reason is that to be
# perfectly correct, this `LD_PRELOAD` library has to be built with the same
# toolchain as the `glibc` that we're interposing.
#
# It's not under `/__fs_image__/rpm/` because it's not actually RPM-specific.
LIBRENAME_SHADOWED_PATH = Path('/__fs_image__/librename_shadowed.so')


def _isolate_yum_dnf(
    yum_dnf: YumDnf, install_root, dummy_dev, protected_path_to_dummy,
):
    'Isolate yum/dnf from the host filesystem.'
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
    return ['bash', '-o', 'pipefail', '-uexc', textwrap.dedent('''\
    # The image needs to have a valid `/dev` so that e.g.  RPM post-install
    # scripts can work correctly (true bug: a script writing a regular file
    # at `/dev/null`).  Unfortunately, the way we are invoking `yum`/`dnf`
    # now, it's not feasible to use `systemd-nspawn`, so we hack it like so:
    install_root={quoted_install_root}
    mkdir -p "$install_root"/dev/
    chown root:root "$install_root"/dev/
    chmod 0755 "$install_root"/dev/
    # The mount must be read-write in case a package like `filesystem` is
    # installed and wants to mutate `/dev/`.  Those changes will be
    # gleefully discarded.
    mount {quoted_dummy_dev} "$install_root"/dev/ -o bind
    mount /dev/null "$install_root"/dev/null -o bind

    {quoted_protected_paths}

    # Also protect potentially non-hermetic files that are not required to
    # exist on the host.  We don't expect these to be written, only read, so
    # failing to protect the non-existent ones is OK.
    for bad_file in \
            {default_conf_file} \
            ~/.rpmrc \
            /etc/rpmrc \
            ~/.rpmmacros \
            /etc/rpm/macros \
            ; do
        if [[ -e "$bad_file" ]] ; then
            mount /dev/null "$bad_file" -o bind
        else
            echo "Not isolating $bad_file -- does not exist" 1>&2
        fi
    done

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
    ''').format(
        prog_name=yum_dnf.value,
        default_conf_file={
            YumDnf.yum: '/etc/yum.conf',
            YumDnf.dnf: '/etc/dnf/dnf.conf',
        }[yum_dnf],
        quoted_dummy_dev=dummy_dev,
        quoted_install_root=install_root.shell_quote(),
        quoted_protected_paths='\n'.join(
            'mount {} {} -o bind,ro'.format(
                dummy.shell_quote(),
                (
                    # Convention: relative for image, or absolute for host.
                    '' if prot_path.startswith(b'/') else '"$install_root"/'
                ) + prot_path.shell_quote(),
            ) for prot_path, dummy in protected_path_to_dummy.items()
        ),
        maybe_set_env_vars=' '.join([
            f'LD_PRELOAD={LIBRENAME_SHADOWED_PATH.shell_quote()}',
            f'FS_IMAGE_SHADOWED_PATHS_ROOT={SHADOWED_PATHS_ROOT.shell_quote()}',
        ]) if os.path.exists(LIBRENAME_SHADOWED_PATH) else '',
    )]

@contextmanager
def _dummy_dev() -> str:
    'A whitelist of devices is safer than the entire host /dev'
    dummy_dev = tempfile.mkdtemp()
    try:
        subprocess.check_call(['sudo', 'chown', 'root:root', dummy_dev])
        subprocess.check_call(['sudo', 'chmod', '0755', dummy_dev])
        subprocess.check_call([
            'sudo', 'touch', os.path.join(dummy_dev, 'null'),
        ])
        yield dummy_dev
    finally:
        # We cannot use `TemporaryDirectory` for cleanup since the directory
        # and contents are owned by root.  Remove recursively since RPMs
        # like `filesystem` can touch this dummy directory.  We will discard
        # their writes, which do not, anyhow, belong in a container image.
        subprocess.run(['sudo', 'rm', '-r', dummy_dev])


@contextmanager
def _dummies_for_protected_paths(
    protected_paths: Iterable[str],
) -> Mapping[Path, Path]:
    '''
    Some locations (some host yum/dnf directories, and install root /meta/
    and mountpoints) should be off-limits to writes by RPMs.  We enforce
    that by bind-mounting an empty file or directory on top of each one.
    '''
    with temp_dir() as td, tempfile.NamedTemporaryFile() as tf:
        # NB: There may be duplicates in protected_paths, so we normalize.
        # If the duplicates include both a file and a directory, this picks
        # one arbitrarily, and if the type on disk is different, we will
        # fail at mount time.  This doesn't seem worth an explicit check.
        yield {
            Path(p).normpath(): (td if p.endswith('/') else Path(tf.name))
                for p in protected_paths
        }
        # NB: The bind mount is read-only, so this is just paranoia.  If it
        # were left RW, we'd need to check its owner / permissions too.
        for expected, actual in (([], td.listdir()), (b'', tf.read())):
            assert expected == actual, \
                f'Some RPM wrote {actual} to {protected_paths}'


def _ensure_fs_image_container():
    '''
    Forbid running this outside of an `fs_image/nspawn_in_subvol` container.
    Since we default to `--installroot=/`, there is some risk to allowing
    execution in other settings.
    '''
    # Any `fs_image` container with snapshots must have `/__fs_image__`
    assert os.path.isdir('/__fs_image__'), \
        '`yum-dnf-from-snapshot` must run in an `nspawn_in_subvol` container'
    # Future: are there other checks we can add?


def _ensure_private_network():
    '''
    Normally, we run under `systemd-nspawn --private-network`.  We don't
    want to run in environments with network access because in these cases
    it's very possible that `yum` / `dnf` will end up doing something
    non-deterministic by reaching out to the network.
    '''
    # From `/usr/include/uapi/linux/if_arp.h`
    allowed_types = {
        768,  # ARPHRD_TUNNEL
        769,  # ARPHRD_TUNNEL6
        772,  # ARPHRD_LOOPBACK
    }
    net = Path('/sys/class/net')
    for iface in net.listdir():
        with open(net / iface / 'type') as infile:
            iface_type = int(infile.read())
            # Not covered because we don't want to rely on the CI container
            # having a network interface.
            if iface_type not in allowed_types:  # pragma: no cover
                raise RuntimeError(
                    'Refusing to run without --private-network, found '
                    f'unknown interface {iface} of type {iface_type}.'
                )


def _install_root(conf_path: Path, yum_dnf_args: Iterable[str]) -> Path:
    # Peek at the `yum` / `dnf` args, which take precedence over the config.
    p = argparse.ArgumentParser(allow_abbrev=False, add_help=False)
    p.add_argument('--installroot', type=Path.from_argparse)
    args, _ = p.parse_known_args(yum_dnf_args)
    if args.installroot:
        return args.installroot
    # For our wrapper to be transparent, the `installroot` semantics have to
    # match that of `yum` / `dnf`, so the argument is optional, with a
    # fallback to the config file, and then to `/`.
    cp = ConfigParser()
    with open(conf_path) as conf_in:
        cp.read_file(conf_in)
    return Path(cp['main'].get('installroot', '/'))


def yum_dnf_from_snapshot(
    *,
    yum_dnf: YumDnf,
    snapshot_dir: Path,
    protected_paths: List[str],
    yum_dnf_args: List[str],
    debug: bool = False,
):
    _ensure_fs_image_container()
    _ensure_private_network()

    prog_name = yum_dnf.value
    # This path convention must match how `write_yum_dnf_conf.py` and
    # `rpm_repo_snapshot.bzl` set up their output.
    conf_path = snapshot_dir / f'{prog_name}/etc/{prog_name}/{prog_name}.conf'
    install_root = _install_root(conf_path, yum_dnf_args)

    # The paths that have trailing slashes are directories, others are
    # files.  There's a separate code path for protecting some files above.
    # The rationale is that those files are not guaranteed to exist.
    protected_paths.extend([
        # See the `_isolate_yum_dnf` docblock for how (and why) this list
        # was produced.  All are assumed to exist on the host -- otherwise,
        # we'd be in the awkard situation of leaving them unprotected, or
        # creating them on the host to protect them.
        '/etc/yum.repos.d/',  # dnf ALSO needs this isolated
        f'/etc/{prog_name}/',   # A duplicate for the `yum` case
        '/etc/pki/rpm-gpg/',
        '/etc/rpm/',
        # Harcode `IMAGE/meta` because it should ALWAYS be off-limits --
        # even though the compiler will redundantly tell us to protect it.
        'meta/',
    ] + (
        # On Fedora, `yum` is just a symlink to `dnf`, so `/etc/yum` is missing
        ['/etc/yum/'] if (has_yum() and not yum_is_dnf()) else []
    ))
    # Only isolate the host DBs and log if we are NOT installing to /.
    install_to_fs_root = install_root.realpath() == b'/'
    if not install_to_fs_root:
        # Ensure the log exists, so we can guarantee we don't write to the host.
        log_path = f'/var/log/{prog_name}.log'
        subprocess.check_call(['sudo', 'touch', log_path])
        protected_paths.extend([
            log_path,
            f'/var/lib/{prog_name}/',
            '/var/lib/rpm/',
            # Future: We should isolate the cache even when installing to /
            # because the snapshot should have its own cache, and should not
            # pollute the OS cache.  However, right now our cache handling
            # is pretty broken, so this change is deferred.
            f'/var/cache/{prog_name}/',
        ])

    for arg in yum_dnf_args:
        assert arg != '-c' and not arg.startswith('--config'), \
            f'If you change --config, you will no longer use the repo snapshot'

    with _dummy_dev() as dummy_dev, \
            _dummies_for_protected_paths(
                protected_paths,
            ) as protected_path_to_dummy, \
            subprocess.Popen([
                'sudo',
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
                'unshare', '--mount', '--uts', '--ipc',
                *_isolate_yum_dnf(
                    yum_dnf, install_root, dummy_dev, protected_path_to_dummy,
                ),
                'yum-dnf-from-snapshot',  # argv[0]
                prog_name,
                # Help us debug CI issues that don't reproduce locally.
                '--verbose',
                # Config options get isolated by our `YumDnfConfIsolator`
                # when `write-yum-dnf-conf` builds this file.  Note that
                # `yum` doesn't work if the config path is relative.
                f'--config={conf_path.abspath()}',
                f'--installroot={install_root}',
                # NB: We omit `--downloaddir` because the default behavior
                # is to put any downloaded RPMs in `$installroot/$cachedir`,
                # which is reasonable, and easy to clean up in a post-pass.
                *yum_dnf_args,
            ]) as yum_dnf_proc:

        # Wait **before** we tear down all the `yum` / `dnf` isolation.
        yum_dnf_proc.wait()
        check_popen_returncode(yum_dnf_proc)


# This argument-parsing logic is covered by RpmActionItem tests.
if __name__ == '__main__':  # pragma: no cover
    main_parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    main_parser.add_argument(
        '--snapshot-dir', required=True, type=Path.from_argparse,
        help='Multi-repo snapshot directory.',
    )
    main_parser.add_argument(
        '--protected-path', action='append', default=[],
        # Future: if desired, the trailing / convention could be relaxed,
        # see `_protected_path_set`.  If so, this program would just need to
        # run `os.path.isdir` against each of the paths.
        help='When `yum` or `dnf` runs, this path will have an empty file or '
            'directory read-only bind-mounted on top. If the path has a '
            'trailing /, it is a directory, otherwise -- a file. If the path '
            'is absolute, it is a host path. Otherwise, it is relative to '
            '`--installroot`. The path must already exist. There are some '
            'internal defaults that cannot be un-protected. May be repeated.',
    )
    main_parser.add_argument('--debug', action='store_true', help='Log more')
    main_parser.add_argument('yum_dnf', type=YumDnf, help='yum or dnf')
    main_parser.add_argument(
        'args', nargs='+',
        help='Pass these through to `yum` or `dnf`. You will want to use -- '
            'before any such argument to prevent `yum-dnf-from-snapshot` '
            'from parsing them. Avoid arguments that might break hermeticity '
            '(e.g. affecting the host system, or making us depend on the '
            'host system) -- this tool implements protections, but it '
            'may not be foolproof.',
    )
    args = main_parser.parse_args()

    init_logging(debug=args.debug)

    yum_dnf_from_snapshot(
        yum_dnf=args.yum_dnf,
        snapshot_dir=args.snapshot_dir,
        protected_paths=args.protected_path,
        yum_dnf_args=args.args,
        debug=args.debug,
    )
