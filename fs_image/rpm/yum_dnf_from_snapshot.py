#!/usr/bin/env python3
'''
This tool wraps `yum` and its successor `dnf`, with two changes to ensure
more hermetic behavior:

  - The `{yum,dnf}.conf`, the repo metadata, and the RPM contents are all
    served out of a directory produced `snapshot-repos`, **not** against
    some kind of live, external RPM repo collection.  So if both the
    snapshot, and `fs_images/rpm` are committed to the same version control,
    then a single version ID completely determines the outcome of a package
    manager invocation.

  - The `yum` or `dnf` invocation runs in a Linux namespaces sandbox with an
    isolated network, and with some additional bind mounts that are intended
    to prevent it from accessing host configuration, state, or caches.

Put in other words, we first configure `yum` or `dnf` to act in a
deterministic fashion, and later execute it in a sandbox to prevent its
non-isolatable features from behaving non-determinitically.

Note that `dnf` support was "bolted on" to this tool, and may be less
mature.  However, the isolation features are no longer very important, since
we now do all RPM installs from a "build appliance", so this code-path will
be significantly simplified soon.

Sample usage:

    buck run TARGET_PATH:yum-dnf-from-snapshot -- \\
        --storage '{"key": "SOME_KEY", "kind": "SOME_KIND", ...}' \\
        --snapshot-dir REPOS_PATH --install-root TARGET_DIR -- \\
            dnf install --assumeyes some-package-name

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

  - `yum` & `dnf` leaves various caches inside `--install-root`, which bloat
    the image.  `RpmActionItem` has a bind-mount to prevent this leakage,
    and we also provide `//fs_image/features:rpm_cleanup` which should be
    included in all production layers.

  - Base CentOS packages deposit a vanilla CentOS `yum` configuration into
    the install root via `/etc/yum.repos.d/`, bearing no relation to the
    `yum.conf` that was used to install packages into the image.  Note that
    `dnf` would try to look in the same `yum.repos.d` if we did not hide it.

  - This is significantly slower than vanilla `yum/dnf`.  On an SSD host, a
    vanilla `yum install net-tools` takes about 1:00, while the current
    `yum-dnf-from-snapshot` needs 3:40.  The two major reasons are:

      * `repo-server` could be faster, specifically it (i) should support
        serving multiple files in parallel, (ii) the Facebook-production
        blob store has some notes on how to eliminate the ~1 second-per-blob
        fetch latency at the expense of 1-2 days of work, (iii) some caching
        of blobs may help, (iv) `repo_server.py` can start faster if
        it avoids eagerly loading the `rpm` table into RAM.

      * Since we typically run `yum/dnf` in an empty clean install-root, the
        initial run can be extra-slow due to having to download the repodata,
        and build the local DB / populate local caches.  However, the
        `RpmActionItem` "build appliance" normally mitigates this by
        providing warm caches for the repo snapshot.

  - This brings up and tears down network namespaces frequently. According
    to ast@kernel.org, bugs are routinely introduced that break NETNS
    clean-up, which may cause us to leak namespaces in production. If this
    becomes an issue, we can try cgroup-bpf style firewalling instead,
    along the lines of the program in `bind4_prog_load` in the kernel's
    `test_sock_addr.c`.

  - When installing into a blank root, `yum/dnf` cannot discover the
    release, so it literally has `/repos/x86_64/$releasever` as the
    'persistdir' subdirectory.  How should we determine the correct release
    for a snapshot-based install?  Fake it?  Add `/etc/*-release` from the
    snapshot host to the snapshot?

We're on the verge of shipping the easy fix of parallelizing `repo-server`
very soon, but other perf improvements will be wanted.
'''
import gzip
import importlib
import os
import shlex
import shutil
import socket
import subprocess
import tempfile
import textwrap
import time

from contextlib import contextmanager
from urllib.parse import urlparse, urlunparse
from typing import List, Mapping, TextIO

from fs_image.common import (
    check_popen_returncode, FD_UNIX_SOCK_TIMEOUT, get_file_logger,
    listen_temporary_unix_socket, recv_fds_from_unix_sock,
)
from fs_image.fs_utils import Path, temp_dir
from .yum_dnf_conf import YumDnf, YumDnfConfParser

log = get_file_logger(__file__)


@contextmanager
def prepare_isolated_yum_dnf_conf(
    yum_dnf: YumDnf, inp: TextIO, out: tempfile.NamedTemporaryFile,
    install_root: Path, host: str, port: int, versionlock_dir: Path,
    config_file_path: Path,
):
    '''
    Reads a "{yum,dnf}.conf" from `inp`, and writes a modified version to
    `out`, installing into `install_root`, and getting packages + GPG keys
    from a snapshot `repo-server` at `http://host:port`.

    This is a context manager because in a prior iteration, the resulting
    isolated "{yum,dnf}.conf" was only valid for as long as some associated
    temporary directories continued to exist.  I'm keeping this contract in
    case we need to do this again in the future.
    '''
    server_url = urlparse(f'http://{host}:{port}')
    yc = YumDnfConfParser(yum_dnf, inp)
    yc.isolate().isolate_repos(
        repo._replace(
            base_url=urlunparse(server_url._replace(path=repo.name)),
            gpg_key_urls=[
                urlunparse(server_url._replace(path=os.path.join(
                    repo.name, os.path.basename(urlparse(key_url).path),
                ))) for key_url in repo.gpg_key_urls
            ],
        ) for repo in yc.gen_repos()
    ).isolate_main(
        install_root=install_root.decode(),
        config_path=config_file_path.decode(),
        versionlock_dir=versionlock_dir.decode(),
    ).write(out)
    out.flush()
    yield  # The config we wrote is valid only inside the context.


@contextmanager
def launch_repo_server(
    repo_server_bin: Path,
    sock: socket.socket,
    storage_cfg: str,
    snapshot_dir: Path
):
    '''
    Invokes `repo-server` with the given storage & snapshot; passes it
    ownership of the bound TCP socket -- it listens & accepts connections.
    '''
    # This could be a thread, but it's probably not worth the risks
    # involved in mixing threads & subprocess (yes, lots of programs do,
    # but yes, far fewer do it safely).
    with sock, subprocess.Popen([
        repo_server_bin,
        '--socket-fd', str(sock.fileno()),
        '--storage', storage_cfg,
        '--snapshot-dir', snapshot_dir,
    ], pass_fds=[sock.fileno()]) as server_proc:
        try:
            yield server_proc
        finally:
            server_proc.kill()  # It's a read-only proxy, abort ASAP


@contextmanager
def _temp_fifo() -> str:
    with tempfile.TemporaryDirectory() as td:
        path = os.path.join(td, 'fifo')
        os.mkfifo(path)
        yield path


def _isolate_yum_dnf_and_wait_until_ready(
    yum_dnf: YumDnf,
    install_root, dummy_dev, protected_path_to_dummy, netns_fifo, ready_fifo,
):
    '''
    Isolate yum/dnf from the host filesystem.  Also, since we have a network
    namespace, we must wait for the parent to set up a socket inside.
    '''
    # Yum is incorrigible -- it is impossible to give it a set of options
    # that will completely prevent it from accessing host configuration &
    # caches.  So instead, we do this:
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
    # This must be open so the parent can `open(ready_fifo, 'w')`, and we
    # must open it before `netns_fifo` not to deadlock.
    exec 3< {quoted_ready_fifo}
    echo -n /proc/$$/ns/net > {quoted_netns_fifo}
    # `yum` & `dnf` will talk to the repo snapshot server via loopback, but
    # it is `down` by default in a new network namespace.
    ifconfig lo up

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

    # Ensure the log exists, so we can guarantee we don't write to the host.
    touch /var/log/{prog_name}.log

    {quoted_protected_paths}

    # Also protect potentially non-hermetic files that are not required to
    # exist on the host.  We don't expect these to be written, only read, so
    # failing to protect the non-existent ones is OK.
    for bad_file in \
            {conf_file} \
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

    # Wait for the repo server to be up.
    if [[ "$(cat <&3)" != ready ]] ; then
        echo 'Did not get ready signal' 1>&2
        exit 1
    fi
    # NB: The `trap` above means the `bash` process is not replaced by the
    # child, but that's not a problem.
    exec "$@"
    ''').format(
        prog_name=yum_dnf.value,
        conf_file={
            YumDnf.yum: '/etc/yum.conf',
            YumDnf.dnf: '/etc/dnf/dnf.conf',
        }[yum_dnf],
        quoted_dummy_dev=dummy_dev,
        quoted_install_root=shlex.quote(install_root.decode()),
        quoted_netns_fifo=shlex.quote(netns_fifo),
        quoted_ready_fifo=shlex.quote(ready_fifo),
        quoted_protected_paths='\n'.join(
            'mount {} {} -o bind,ro'.format(
                shlex.quote(dummy),
                (
                    # Convention: relative for image, or absolute for host.
                    '' if prot_path.startswith('/') else '"$install_root"/'
                ) + shlex.quote(prot_path),
            ) for prot_path, dummy in protected_path_to_dummy.items()
        ),
    )]


def _make_socket_and_send_via(*, unix_sock_fd):
    '''
    Creates a TCP stream socket and sends it elsewhere via the provided Unix
    domain socket file descriptor.  This is useful for obtaining a socket
    that belongs to a different network namespace (i.e.  creating a socket
    inside a container, but binding it from outside the container).

    IMPORTANT: This code must not write anything to stdout, the fd can be 1.
    '''
    # NB: Some code here is (sort of) copy-pasta'd in `send_fds_and_run.py`,
    # but it's not obviously worthwhile to reuse it here.
    return ['python3', '-c', textwrap.dedent('''\
    import array, socket, sys

    def send_fds(sock, msg: bytes, fds: 'List[int]'):
        num_sent = sock.sendmsg([msg], [(
            socket.SOL_SOCKET, socket.SCM_RIGHTS,
            array.array('i', fds).tobytes(),
            # Future: is `flags=socket.MSG_NOSIGNAL` a good idea?
        )])
        assert len(msg) == num_sent, (msg, num_sent)

    # Make a socket in this netns, and send it to the parent.
    with socket.socket(fileno=''' + str(unix_sock_fd) + ''') as lsock:
        print(f'Sending FD to parent', file=sys.stderr)
        lsock.settimeout(''' + str(FD_UNIX_SOCK_TIMEOUT) + ''')
        with lsock.accept()[0] as csock, socket.socket(
            socket.AF_INET, socket.SOCK_STREAM
        ) as inet_sock:
            csock.settimeout(''' + str(FD_UNIX_SOCK_TIMEOUT) + ''')
            send_fds(csock, b'ohai', [inet_sock.fileno()])
    ''')]


def create_socket_inside_netns(netns_path):
    '''
    Creates TCP stream socket inside the container.

    Returns the socket.socket() object.
    '''
    with listen_temporary_unix_socket() as (
        unix_sock_path, list_sock
    ), subprocess.Popen([
        # NB: /usr/local/fbcode/bin must come first because /bin/python3
        # may be very outdated
        'sudo', 'env', 'PATH=/usr/local/fbcode/bin:/bin',
        'nsenter', f'--net={netns_path}',
        # NB: We pass our listening socket as FD 1 to avoid dealing with
        # the `sudo` option of `-C`.  Nothing here writes to `stdout`:
        *_make_socket_and_send_via(unix_sock_fd=1),
    ], stdout=list_sock.fileno()) as sock_proc:
        repo_server_sock_fd, = recv_fds_from_unix_sock(unix_sock_path, 1)
        repo_server_sock = socket.socket(fileno=repo_server_sock_fd)
    check_popen_returncode(sock_proc)
    return repo_server_sock


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
def _dummies_for_protected_paths(protected_paths) -> Mapping[str, str]:
    '''
    Some locations (e.g. /meta/ and mountpoints) should be off-limits to
    writes by RPMs.  We enforce that by bind-mounting an empty file or
    directory on top of each one of them.
    '''
    with tempfile.TemporaryDirectory() as td_name, \
            tempfile.NamedTemporaryFile() as tf:
        # NB: There may be duplicates in protected_paths, so we normalize.
        # If the duplicates include both a file and a directory, this picks
        # one arbitrarily, and if the type on disk is different, we will
        # fail at mount time.  This doesn't seem worth an explicit check.
        yield {
            os.path.normpath(p): (td_name if p.endswith('/') else tf.name)
                for p in protected_paths
        }
        # NB: The bind mount is read-only, so this is just paranoia.  If it
        # were left RW, we'd need to check its owner / permissions too.
        for expected, actual in (
            ([], os.listdir(td_name)),
            (b'', tf.read()),
        ):
            assert expected == actual, \
                f'Some RPM wrote {actual} to {protected_paths}'


@contextmanager
def prepare_versionlock_dir(yum_dnf: YumDnf, list_path: Path) -> Path:
    '''
    This prepares a directory containing:
      - the versionlock plugin code (see the Buck target for its provenance)
      - the plugin configuration
      - the actual list of locked versions

    This directory is used by `YumDnfConfIsolator.isolate_main` to tell
    `yum` / `dnf` to use the plugin.
    '''
    with temp_dir() as d:
        vl_conf = textwrap.dedent(f'''\
            [main]
            enabled = 1
            locklist = {d.decode()}/versionlock.list
        ''')
        with open(d / 'versionlock.conf', 'w') as outf:
            outf.write(vl_conf)

        # `dnf` and `yum` expect different formats, so we parse our own.
        template = {
            YumDnf.yum: '{e}:{n}-{v}-{r}.{a}',
            YumDnf.dnf: '{n}-{e}:{v}-{r}.{a}',
        }[yum_dnf]
        with open(list_path) as rf, open(d / 'versionlock.list', 'w') as wf:
            for l in rf:
                e, n, v, r, a = l.split('\t')
                wf.write(template.format(e=e, n=n, v=v, r=r, a=a))

        with importlib.resources.path(
            'rpm', f'{yum_dnf.value}_versionlock.gz',
        ) as p, gzip.open(p) as rf, open(d / 'versionlock.py', 'wb') as wf:
            wf.write(rf.read())

        yield d

        # Clean up, making sure that there are no new files.

        # Comparing the contents of the plugin & its list is too much effort
        with open(d / 'versionlock.conf') as infile:
            assert infile.read() == vl_conf

        assert (set(os.listdir(d)) - {b'versionlock.pyc', b'__pycache__'}) == {
            b'versionlock.conf', b'versionlock.list', b'versionlock.py',
        }, os.listdir(d)


def yum_dnf_from_snapshot(
    *,
    yum_dnf: YumDnf,
    repo_server_bin: Path,
    storage_cfg: str,
    snapshot_dir: Path,
    install_root: Path,
    protected_paths: List[str],
    versionlock_list: Path,
    yum_dnf_args: List[str],
):
    prog_name = yum_dnf.value
    # The paths that have trailing slashes are directories, others are
    # files.  There's a separate code path for protecting some files above.
    # The rationale is that those files are not guaranteed to exist.
    protected_paths.extend([
        f'/var/log/{prog_name}.log',  # Created above if it doesn't exist
        # See the `_isolate_yum_dnf_and_wait_until_ready` docblock for how
        # (and why) this list was produced.  All are assumed to exist on the
        # host -- otherwise, we'd be in the awkard situation of leaving them
        # unprotected, or creating them on the host to protect them.
        '/etc/yum.repos.d/',  # dnf ALSO needs this isolated
        '/etc/yum/',  # dnf ALSO needs this isolated
        f'/etc/{prog_name}/',   # A duplicate for the `yum` case
        f'/var/cache/{prog_name}/',
        f'/var/lib/{prog_name}/',
        '/etc/pki/rpm-gpg/',
        '/etc/rpm/',
        '/var/lib/rpm/',
        # Harcode `IMAGE/meta` because it should ALWAYS be off-limits --
        # even though the compiler will redundantly tell us to protect it.
        'meta/',
    ])

    # These user-specified arguments could really mess up hermeticity.
    for bad_arg in ['--installroot', '--config', '--setopt', '--downloaddir']:
        for arg in yum_dnf_args:
            assert arg != '-c'
            assert not arg.startswith(bad_arg), f'{arg} is prohibited'

    with _temp_fifo() as netns_fifo, _temp_fifo(
                # Lets the child wait for `out_conf` to be ready. This could
                # be done via an `flock` on `out_conf.name`, but that's not
                # robust on some network filesystems, so let's use a pipe.
            ) as ready_fifo, \
            tempfile.NamedTemporaryFile('w', suffix=prog_name) as out_conf, \
            _dummy_dev() as dummy_dev, \
            _dummies_for_protected_paths(
                protected_paths,
            ) as protected_path_to_dummy, \
            subprocess.Popen([
                'sudo',
                # Cannot do --pid or --cgroup without extra work (nspawn).
                # Note that `--mount` implies `mount --make-rprivate /` for
                # all recent `util-linux` releases (since 2.27 circa 2015).
                'unshare', '--mount', '--uts', '--ipc', '--net',
                *_isolate_yum_dnf_and_wait_until_ready(
                    yum_dnf, install_root, dummy_dev,
                    protected_path_to_dummy, netns_fifo, ready_fifo,
                ),
                'yum-dnf-from-snapshot',  # argv[0]
                prog_name,
                # Config options are isolated by our `YumDnfConfIsolator`.
                '--config', out_conf.name,
                # NB: We omit `--downloaddir` because the default behavior
                # is to put any downloaded RPMs in `$installroot/$cachedir`,
                # which is reasonable, and easy to clean up in a post-pass.
                *yum_dnf_args,
            ]) as yum_dnf_proc, \
            open(
                # ORDER IS IMPORTANT: In case of error, this must be closed
                # before `proc.__exit__` calls `wait`, or we'll deadlock.
                ready_fifo, 'w'
            ) as ready_out:

        # To start the repo server we must obtain a socket that belongs to
        # the network namespace of the `yum` / `dnf` container, and we must
        # bring up the loopback device to later bind to it.  Since this
        # parent process has low privileges, we do this via a `sudo` helper.
        with open(netns_fifo, 'r') as netns_in:
            netns_path = netns_in.read()

        repo_server_sock = create_socket_inside_netns(netns_path)

        # Binds the socket to the loopback inside yum/dnf's netns
        repo_server_sock.bind(('127.0.0.1', 0))
        host, port = repo_server_sock.getsockname()
        log.info(f'Bound {netns_path} socket to {host}:{port}')

        # The server takes ownership of the socket, so we don't enter it here.
        with launch_repo_server(
            repo_server_bin, repo_server_sock, storage_cfg, snapshot_dir
        ) as server_proc, \
                open(snapshot_dir / f'{prog_name}.conf') as in_conf, \
                prepare_versionlock_dir(
                    yum_dnf, versionlock_list,
                ) as versionlock_td, \
                prepare_isolated_yum_dnf_conf(
                    yum_dnf, in_conf, out_conf, install_root, host, port,
                    versionlock_td, Path(out_conf.name),
                ):

            log.info('Waiting for repo server to listen')
            while server_proc.poll() is None:
                if repo_server_sock.getsockopt(
                    socket.SOL_SOCKET, socket.SO_ACCEPTCONN,
                ):
                    break
                time.sleep(0.1)

            log.info(f'Ready to run {prog_name}')
            ready_out.write('ready')  # `yum` / `dnf` can run now.
            ready_out.close()  # Proceed past the inner `read`.

            # Wait **before** we tear down all the `yum/dnf.conf` isolation.
            yum_dnf_proc.wait()
            check_popen_returncode(yum_dnf_proc)


# This argument-parsing logic is covered by RpmActionItem tests.
if __name__ == '__main__':  # pragma: no cover
    import argparse
    import json

    from .common import init_logging

    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        '--repo-server', required=True, type=Path.from_argparse,
        help='Path to repo-server binary',
    )
    parser.add_argument(
        '--snapshot-dir', required=True, type=Path.from_argparse,
        help='Multi-repo snapshot directory.',
    )
    parser.add_argument(
        '--storage', required=True, type=json.loads,
        help='What Storage do the storage IDs of the snapshots refer to? '
            'Run `repo-server --help` to learn the syntax.',
    )
    parser.add_argument(
        '--install-root', required=True, type=Path.from_argparse,
        help='All packages will be installed under this root. This is '
            'identical to the underlying `--installroot` option, but it '
            'is required here because most users of `yum-dnf-from-snapshot` '
            'should not install to /.',
    )
    parser.add_argument(
        '--protected-path', action='append', default=[],
        # Future: if desired, the trailing / convention could be relaxed,
        # see `_protected_path_set`.  If so, this program would just need to
        # run `os.path.isdir` against each of the paths.
        help='When `yum` or `dnf` runs, this path will have an empty file or '
            'directory read-only bind-mounted on top. If the path has a '
            'trailing /, it is a directory, otherwise -- a file. If the path '
            'is absolute, it is a host path. Otherwise, it is relative to '
            '--install-root. The path must already exist. There are some '
            'internal defaults that cannot be un-protected. May be repeated.',
    )
    parser.add_argument(
        '--versionlock-list', default='/dev/null',
        help='A file listing allowed RPM versions, one per line, in the '
            'following TAB-separated format: N\\tE\\tV\\tR\\tA.',
    )
    parser.add_argument('yum_dnf', type=YumDnf, help='yum or dnf')
    parser.add_argument(
        'args', nargs='+',
        help='Pass these through to `yum` or `dnf`. You will want to use -- '
            'before any such argument to prevent `yum-dnf-from-snapshot` '
            'from parsing them. Avoid arguments that might break hermeticity '
            '(e.g. affecting the host system, or making us depend on the '
            'host system) -- this tool implements protections, but it '
            'may not be foolproof.',
    )
    args = parser.parse_args()

    init_logging()

    yum_dnf_from_snapshot(
        yum_dnf=args.yum_dnf,
        repo_server_bin=args.repo_server,
        storage_cfg=json.dumps(args.storage),
        snapshot_dir=args.snapshot_dir,
        install_root=args.install_root,
        protected_paths=args.protected_path,
        versionlock_list=args.versionlock_list,
        yum_dnf_args=args.args,
    )
