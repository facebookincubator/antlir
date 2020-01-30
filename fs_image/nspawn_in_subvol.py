#!/usr/bin/env python3
'''
When developing images, it is very handy to be able to run code inside an
image.  This target lets you do just that, for example, here is a shell:

    buck run //fs_image:nspawn-run-in-subvol -- --layer "$(
        buck build --show-output \\
            //fs_image/compiler/tests:only-for-tests-read-only-host-clone |
                cut -f 2- -d ' ')"

The above is a handful to remember, so each layer gets a corresponding
`-container` target.  To be used like so:

    buck run //PATH/TO:SOME_LAYER-container  # Runs `bash` by default
    buck run //PATH/TO:SOME_LAYER-container -- -- printenv

Note that there are two sets of `--`.  The first separates `buck run`
arguments from those of the container runtime.  The second separates the
container args from the in-container command.

Note: If no command is passed to systemd-nspawn to execute, then the
defualt behavior is to invoke a shell. `/bin/bash` is tried first and if
that is not found then `/bin/sh` is used.  We use this default behavior
to provide a shell as the default when `buck run` is used as in the
example above.

IMPORTANT: This is NOT READY to use as a sandbox for build steps.  The
reason is that `systemd-nspawn` does a bunch of random things to the
filesystem, which we would need to explicitly control (see "Filesystem
mutations" below).


## Known issues

  - The `hostname` of the container is not currently set to a useful value,
    which can affect some network operations.

  - T40937041: If `stdout` is a PTY, then `stderr` redirection does not work
    -- the container's `stderr` will also point at the PTY.  This is an
    nspawn bug, and working around it form this wrapper would be hard.  This
    issue was fixed in systemd 242.

  - T40936918: At present, `nspawn` prints a spurious newline to stdout,
    even if `stdout` is redirected.  This is due to an errant `putc('\\n',
    stdout);` in `nspawn.c`.  This will most likely be fixed in future
    releases of systemd.  I could work around this in the wrapper by passing
    `--quiet` when `not sys.stdout.isatty()`.  However, that loses valuable
    debugging output, so I'm not doing it yet.  This issue was fixed in
    systemd 242.


## What does nspawn do, roughly?

This section is as of systemd 238/239, and will never be 100% perfect.  For
production-readiness, we would want to write automatic tests of nspawn's
behavior (especially, against minimal containers) to ensure future `systemd`
releases don't surprise us by creating yet-more filesystem objects.


### Isolates all specified kernel namespaces

  - pid
  - mount
  - network with --private-network
  - uts & ipc
  - cgroup (if supported by the base system)
  - user (if requested, we don't request it below due to kernel support)


### Filesystem mutations and requirements

`nspawn` will refuse to use a directory unless these two exist:
  - `/usr/`
  - an `os-release` file

`nspawn` will always ensure these exist before starting its container:
  - /dev
  - /etc
  - /lib will symlink to /usr/lib if the latter exists, but the former does not
  - /proc
  - /root -- permissions nonstandard, should be 0700 not 0755.
  - /run
  - /sys
  - /tmp
  - /var/log/journal/

`nspawn` wants to modify `/etc/resolv.conf` if `--private-network` is off.

The permissions of the created directories seem to be 0755 by default, and
all are owned by root (except for $HOME which may depend if we vary the
user, which we should probably never do).


## Future

  - Should we drop CAP_NET_ADMIN, or any other capabilities?  Note that
    NET_ADMIN might be needed to set up `--private-network` interfaces.

  - Can we get any mileage out of --system-call-filter?

'''
import argparse
import functools
import logging
import os
import pwd
import re
import signal
import subprocess
import sys
import tempfile
import textwrap
import uuid

from contextlib import contextmanager

from artifacts_dir import find_repo_root
from compiler import procfs_serde
from find_built_subvol import find_built_subvol, Subvol
from fs_image.common import (
    init_logging,
    nullcontext,
    pipe,
)
from fs_image.compiler.items.mount_utils import clone_mounts
from fs_image.fs_utils import Path
from rpm.yum_dnf_from_snapshot import (
    create_socket_inside_netns,
    launch_repo_server,
    prepare_isolated_yum_dnf_conf,
)
from rpm.yum_dnf_conf import YumDnf
from send_fds_and_run import popen_and_inject_fds_after_sudo
from tests.temp_subvolumes import TempSubvolumes
from typing import AnyStr, List


DEFAULT_SHELL = '/bin/bash'

REPO_SERVER_CONFIG_DIR = Path('/repo-server')


def _colon_quote_path(path):
    return re.sub('[\\\\:]', lambda m: '\\' + m.group(0), path)


# NB: This assumes the path is readable to unprivileged users.
def _exists_in_image(subvol, path):
    return os.path.exists(subvol.path(path))


def bind_args(src, dest=None, *, readonly=True):
    'dest is relative to the nspawn container root'
    if dest is None:
        dest = src
    # NB: The `systemd-nspawn` docs claim that we can add `:norbind` to make
    # the bind mount non-recursive.  This would be a bad default, so we
    # don't do it, but if you wanted to add it a non-recursive option, be
    # sure to test that nspawn actually implements the functionality -- it's
    # not very obvious from the code that it does (as of 8f6b442a7).
    return [
        '--bind-ro' if readonly else '--bind',
        f'{_colon_quote_path(src)}:{_colon_quote_path(dest)}',
    ]


def _inject_os_release_args(subvol):
    '''
    nspawn requires os-release to be present as a "sanity check", but does
    not use it.  We do not want to block running commands on the image
    before it is created, so make a fake.
    '''
    os_release_paths = ['/usr/lib/os-release', '/etc/os-release']
    for path in os_release_paths:
        if _exists_in_image(subvol, path):
            return []
    # Not covering this with tests because it requires setting up a new test
    # image just for this case.  If we supported nested bind mounts, that
    # would be easy, but we do not.
    return bind_args('/dev/null', os_release_paths[0])  # pragma: no cover


def _nspawn_version():
    '''
    We now care about the version of nspawn we are running.  The output of
    systemd-nspawn --version looks like:

    ```
    systemd 242 (v242-2.fb1)
    +PAM +AUDIT +SELINUX +IMA ...
    ```
    So we can get the major version as the second token of the first line.
    We hope that the output of systemd-nspawn --version is stable enough
    to keep parsing it like this.
    '''
    return int(subprocess.check_output([
        'systemd-nspawn', '--version']).split()[1])


def _nspawn_cmd(nspawn_subvol):
    return [
        # Without this, nspawn would look for the host systemd's cgroup setup,
        # which breaks us in continuous integration containers, which may not
        # have a `systemd` in the host container.
        #
        # We set this variable via `env` instead of relying on the `sudo`
        # configuration because it's important that it be set.
        'env', 'UNIFIED_CGROUP_HIERARCHY=yes',
        'systemd-nspawn',
        # These are needed since we do not want to require a working `dbus` on
        # the host.
        '--register=no', '--keep-unit',
        # Randomize --machine so that the container has a random hostname
        # each time. The goal is to help detect builds that somehow use the
        # hostname to influence the resulting image.
        '--machine', uuid.uuid4().hex,
        '--directory', nspawn_subvol.path(),
        *_inject_os_release_args(nspawn_subvol),
        # Don't pollute the host's /var/log/journal
        '--link-journal=no',
        # Explicitly do not look for any settings for our ephemeral machine
        # on the host.
        '--settings=no',
        # Test containers probably should not be accessing host devices, so
        # take that away until proven necessary.
        '--drop-capability=CAP_MKNOD',
        # The timezone should be set up explicitly, not by nspawn's fiat.
        '--timezone=off',  # requires v239+
        # Future: Uncomment.  This is good container hygiene.  It had to go
        # since it breaks XAR binaries, which rely on a setuid bootstrap.
        # '--no-new-privileges=1',
    ]


def _wrap_systemd_exec():
    return [
        '/bin/bash', '-eu', '-o', 'pipefail', '-c',
        # This script will be invoked with a writable FD forwarded into the
        # namespace this is being executed in as fd #3.
        #
        # It will then get the parent pid of the 'grep' process, which will be
        # the pid of the script itself (running as PID 1 inside the namespace),
        # so eventually the pid of systemd.
        #
        # We don't close the forwarded FD in this script. Instead we rely on
        # systemd to close all FDs it doesn't know about during its
        # initialization sequence.
        #
        # We rely on this because systemd will only close FDs after it creates
        # the /run/systemd/private socket (which makes systemctl usable) and
        # after setting up the necessary signal handlers to process the
        # SIGRTMIN+3 shutdown signal that we need to shut down the container
        # after invoking a command inside it.
        textwrap.dedent('''\
            grep ^PPid: /outerproc/self/status >&3
            umount -R /outerproc
            rmdir /outerproc
            exec /usr/lib/systemd/systemd
        '''),
    ]


def _exfiltrate_container_pid_and_wait_for_ready_cmd(exfil_fd, ready_fd):
    return [
        '/bin/bash', '-eu', '-o', 'pipefail', '-c',
        # This script will exfiltrate the outer PID of a process inside the
        # container's namespace. (See _wrap_systemd_exec for more details.)
        #
        # After sending this information, it will block on the "ready" FD to
        # wait for this script to complete setup.  Once the "ready" signal is
        # received, it will continue to execute the final command.
        textwrap.dedent(f'''\
            grep ^PPid: /outerproc/self/status >&{exfil_fd}
            umount -R /outerproc
            rmdir /outerproc
            exec {exfil_fd}>&-
            read line <&{ready_fd}
            if [[ "$line" != "ready" ]] ; then
                echo 'Did not get ready signal' 1>&2
                exit 1
            fi
            exec {ready_fd}<&-
            exec "$@"
        '''),
        # pass 'bash' as $0, then opts.cmd will become the additional args in
        # $@ for the final `exec` command in the script above.
        'bash',
    ]


@contextmanager
def _exfiltrate_container_pid_and_wait_for_ready(
    nspawn_cmd, container_cmd, forward_fds, popen_for_nspawn,
    repo_server_config
):
    cmd = nspawn_cmd[:]

    # Create a pipe that we can forward into the namespace that our
    # shell script can use to exfil data about the namespace we've
    # been put into before we hand control over to the init system.
    #
    # And a pipe that we can use to signal the bash script that it
    # should go ahead and exec the final command.
    with pipe() as (exfil_r, exfil_w), pipe() as (ready_r, ready_w):

        # We'll add the read end of the pipe to the end of forward_fds,
        # which will then start at FD (3 + len(opts.forward_fd)) inside
        # the subprocess.
        forward_fds = forward_fds[:]
        exfil_fd = 3 + len(forward_fds)
        ready_fd = 4 + len(forward_fds)
        forward_fds.extend([exfil_w.fileno(), ready_r.fileno()])

        cmd.extend([
            f'--bind-ro={repo_server_config.decode()}'
            f':{REPO_SERVER_CONFIG_DIR.decode()}',
            '--bind-ro=/proc:/outerproc',
            '--',
        ])
        cmd.extend(
            _exfiltrate_container_pid_and_wait_for_ready_cmd(
                exfil_fd, ready_fd)
        )
        cmd.extend(container_cmd)
        with popen_and_inject_fds_after_sudo(
            cmd, forward_fds, popen_for_nspawn, set_listen_fds=True
        ) as cmd_proc:
            exfil_w.close()
            ready_r.close()

            # outer PID of a process inside the container.
            container_pid = int(exfil_r.read().decode().split(':')[1].strip())
            exfil_r.close()

            ready_sent = False

            def send_ready():
                nonlocal ready_sent
                if ready_sent:
                    raise RuntimeError(  # pragma: no cover
                        "Can't send ready twice"
                    )
                ready_w.write(b'ready\n')
                ready_w.close()
                ready_sent = True

            try:
                yield container_pid, cmd_proc, send_ready
            finally:
                if not ready_sent:
                    send_ready()  # pragma: no cover


def bind_socket_inside_netns(sock):
    # Binds the socket to the loopback inside yum's netns
    sock.bind(('127.0.0.1', 0))
    host, port = sock.getsockname()
    logging.info(
        f'Bound socket inside netns to {host}:{port}'
    )
    return host, port


def _get_repo_server_storage_config(snapshot_dir):
    with open(snapshot_dir / b'storage.json') as f:
        return f.read()


@contextmanager
def _write_yum_or_dnf_configs(
    yum_dnf, repo_server_config_dir, repo_server_snapshot_dir, host, port
):
    config_filename = {
        YumDnf.yum: Path('yum.conf'),
        YumDnf.dnf: Path('dnf.conf'),
    }[yum_dnf]
    plugin_directory = {
        YumDnf.yum: Path('yum/pluginconf.d'),
        YumDnf.dnf: Path('dnf/plugins'),
    }[yum_dnf]
    os.makedirs(repo_server_config_dir / plugin_directory)
    with open(
        repo_server_snapshot_dir / config_filename
    ) as in_conf, open(
        repo_server_config_dir / config_filename, 'w'
    ) as out_conf, prepare_isolated_yum_dnf_conf(
        yum_dnf,
        in_conf,
        out_conf,
        Path('/'),
        host,
        port,
        REPO_SERVER_CONFIG_DIR / plugin_directory,
        REPO_SERVER_CONFIG_DIR / config_filename,
    ):
        yield


@contextmanager
def _write_yum_and_dnf_configs(
    repo_server_config_dir, repo_server_snapshot_dir, host, port
):
    with _write_yum_or_dnf_configs(
        YumDnf.yum,
        repo_server_config_dir,
        repo_server_snapshot_dir,
        host,
        port,
    ), _write_yum_or_dnf_configs(
        YumDnf.dnf,
        repo_server_config_dir,
        repo_server_snapshot_dir,
        host,
        port,
    ):
        yield


@contextmanager
def _popen_and_inject_repo_server(
    nspawn_cmd, container_cmd, forward_fds, popen_for_nspawn,
    repo_server_snapshot_dir, *, debug
):
    # We're running a repo-server with a socket inside the network
    # namespace.

    with tempfile.TemporaryDirectory() as repo_server_config_dir:
        repo_server_config_dir = Path(repo_server_config_dir)
        with _exfiltrate_container_pid_and_wait_for_ready(
            nspawn_cmd, container_cmd, forward_fds, popen_for_nspawn,
            repo_server_config_dir
        ) as (container_pid, cmd_proc, send_ready):

            repo_server_sock = create_socket_inside_netns(
                f'/proc/{container_pid}/ns/net'
            )
            logging.info(
                f'Got socket at FD {repo_server_sock.fileno()}'
            )
            # Binds the socket to the loopback inside yum's netns
            host, port = bind_socket_inside_netns(repo_server_sock)

            with launch_repo_server(
                os.path.join(repo_server_snapshot_dir, Path('repo-server')),
                repo_server_sock,
                _get_repo_server_storage_config(repo_server_snapshot_dir),
                repo_server_snapshot_dir,
                debug=debug,
            ), _write_yum_and_dnf_configs(
                repo_server_config_dir,
                repo_server_snapshot_dir,
                host,
                port,
            ):
                send_ready()
                yield cmd_proc


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
        if var.startswith('SYSTEMD_NSPAWN_'):  # pragma: no cover
            env.pop(var)
    return env


@contextmanager
def _snapshot_subvol(src_subvol, snapshot_into):
    if snapshot_into:
        nspawn_subvol = Subvol(snapshot_into)
        nspawn_subvol.snapshot(src_subvol)
        clone_mounts(src_subvol, nspawn_subvol)
        yield nspawn_subvol
    else:
        with TempSubvolumes() as tmp_subvols:
            # To make it easier to debug where a temporary subvolume came
            # from, make make its name resemble that of its source.
            tmp_name = os.path.normpath(src_subvol.path())
            tmp_name = os.path.basename(os.path.dirname(tmp_name)) or \
                os.path.basename(tmp_name)
            nspawn_subvol = tmp_subvols.snapshot(src_subvol, tmp_name)
            clone_mounts(src_subvol, nspawn_subvol)
            yield nspawn_subvol


class BootedCompletedProcess(subprocess.CompletedProcess):
    def __init__(self, boot_proc, args, returncode, stdout, stderr):
        self.boot = boot_proc
        super().__init__(
            args=args,
            returncode=returncode,
            stdout=stdout,
            stderr=stderr
        )


def _create_extra_nspawn_args(
    opts, pw, *, artifacts_may_require_repo: bool,
) -> List[AnyStr]:
    if opts.boot:
        extra_nspawn_args = ['--user=root']
    elif opts.repo_server_snapshot_dir:
        # Same as when --boot is passed, running the repo-server requires
        # exfiltrating the PID, and that currently requires root (in order to
        # unmount and rmdir /outerproc), so let's enforce that here.
        extra_nspawn_args = ['--as-pid2', '--user=root']
    else:
        # If we are not booting nspawn run as PID 2. Some commands
        # we run directly will not work correctly as PID 1.
        extra_nspawn_args = ['--as-pid2', f'--user={opts.user}']

    if opts.quiet:
        extra_nspawn_args.append('--quiet')

    if opts.private_network:
        extra_nspawn_args.append('--private-network')

    if opts.bindmount_rw:
        for src, dest in opts.bindmount_rw:
            extra_nspawn_args.extend(bind_args(src, dest, readonly=False))

    if opts.bindmount_ro:
        for src, dest in opts.bindmount_ro:
            extra_nspawn_args.extend(bind_args(src, dest, readonly=True))

    if opts.bind_repo_ro or artifacts_may_require_repo:
        # NB: Since this bind mount is only made within the nspawn
        # container, it is not visible in the `--snapshot-into` filesystem.
        # This is a worthwhile trade-off -- it is technically possible to
        # reimplement this kind of transient mount outside of the nspawn
        # container.  But, by making it available in the outer mount
        # namespace, its unmounting would become unreliable, and handling
        # that would add a bunch of complex code here.
        extra_nspawn_args.extend(bind_args(find_repo_root(sys.argv[0])))
        # Future: we **may** also need to mount the scratch directory
        # pointed to by `buck-image-out`, since otherwise repo code trying
        # to access other built layers won't work.  Not adding it now since
        # that seems like a rather esoteric requirement for the sorts of
        # code we should be running under `buck test` and `buck run`.  NB:
        # As of this writing, `mkscratch` works incorrectly under `nspawn`,
        # making `artifacts-dir` fail.

    if opts.logs_tmpfs:
        extra_nspawn_args.extend(['--tmpfs=/logs:' + ','.join([
            f'uid={pw.pw_uid}', f'gid={pw.pw_gid}', 'mode=0755', 'nodev',
            'nosuid', 'noexec',
        ])])

    # Future: This is definitely not the way to go for providing device
    # nodes, but we need `/dev/fuse` right now to run XARs.  Let's invent a
    # systematic story later.  This cannot be an `image.feature` because of
    # the way that `nspawn` sets up `/dev`.
    #
    # Don't require coverage in case any weird test hosts lack FUSE.
    if os.path.exists('/dev/fuse'):  # pragma: no cover
        extra_nspawn_args.extend(['--bind-ro=/dev/fuse'])

    if opts.cap_net_admin:
        extra_nspawn_args.append('--capability=CAP_NET_ADMIN')

    if opts.hostname:
        extra_nspawn_args.append(f'--hostname={opts.hostname[0]}')

    if opts.forward_tls_env:
        # Add the thrift vars to the user supplied env vars at the beginning
        # of the list so that if the user overides them, their version wins.
        for k, v in os.environ.items():
            if k.startswith('THRIFT_TLS_'):
                opts.setenv.insert(0, f'{k}={v}')

    return extra_nspawn_args


def _run_non_booted_nspawn(
    nspawn_cmd, opts, version, popen
) -> subprocess.CompletedProcess:
    # This is last to let the user have final say over the environment.
    cmd = nspawn_cmd[:]
    cmd.extend(['--setenv=' + se for se in opts.setenv])
    if version >= 242 and opts.cmd[0] != DEFAULT_SHELL:
        # If we have a cmd to pass to nspawn then lets tell nspawn to
        # use the --pipe option.  This will bite us if someone tries to
        # run an interactive repl or directly invoke a shell that is not
        # the default.
        cmd.append('--pipe')

    with (
        _popen_and_inject_repo_server(
            cmd,
            opts.cmd,
            opts.forward_fd,
            popen,
            opts.repo_server_snapshot_dir,
            debug=opts.debug,
        ) if opts.repo_server_snapshot_dir
        else (
            popen_and_inject_fds_after_sudo(
                cmd + ['--'] + opts.cmd,
                opts.forward_fd,
                popen,
                set_listen_fds=True,
            ) if opts.forward_fd
            else popen(cmd + ['--'] + opts.cmd)
        )
    ) as cmd_proc:
        cmd_stdout, cmd_stderr = cmd_proc.communicate()

    return subprocess.CompletedProcess(
        args=cmd_proc.args,
        returncode=cmd_proc.returncode,
        stdout=cmd_stdout,
        stderr=cmd_stderr,
    )


def _run_booted_nspawn(
    nspawn_cmd, opts, pw, nspawn_subvol, popen
) -> subprocess.CompletedProcess:
    # Instead of using the `--boot` argument to `systemd-nspawn` we are
    # going to ask systemd-nspawn to invoke a simple shell script so
    # that we can exfiltrate the process id of the process. After
    # sending that information out, the shell script execs systemd.
    cmd = nspawn_cmd[:]
    cmd.extend([
        '--bind-ro=/proc:/outerproc',
        '--',
        *_wrap_systemd_exec(),
    ])

    # Create a partial of the popen with stdout/stderr setup as
    # requested for the boot process.
    boot_popen = functools.partial(popen,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    # Create a pipe that we can forward into the namespace that our
    # shell script can use to exfil data about the namespace we've been
    # put into before we hand control over to the init system.
    exfil_r, exfil_w = os.pipe()
    with (
        popen_and_inject_fds_after_sudo(
            cmd,
            [exfil_w],  # Forward the write fd of the pipe
            boot_popen,
            set_listen_fds=True
        )
    ) as boot_proc:
        # Close the write fd of the pipe from this process so we
        # can read from this side.
        os.close(exfil_w)

        with os.fdopen(exfil_r, 'r') as f:
            systemd_pid = f.read().split(':')[1].strip()

        # A set of default environment variables that should be
        # setup for the command inside the container.  This list models
        # what the default env looks for a shell launched inside
        # of systemd-nspawn.
        default_env = {
            'HOME': pw.pw_dir,
            'LOGNAME': opts.user,
            'PATH': '/usr/local/bin:/usr/bin:/usr/local/sbin:/usr/sbin',
            'USER': opts.user,
            'TERM': os.environ.get('TERM')
        }
        for k, v in default_env.items():
            opts.setenv.append(f'{k}={v}')

        # Set the user properly for the nsenter'd command to run.
        # Future: consider properly logging in as the user with su
        # or something better so that a real user session is created
        # within the booted container.
        nsenter_cmd = ['nsenter', f'--target={systemd_pid}', '--all',
            f'--setuid={pw.pw_uid}', f'--setgid={pw.pw_gid}',
            # Clear and set the new env
            'env', '-',
            *opts.setenv,
            *opts.cmd]

        with (
            popen_and_inject_fds_after_sudo(
                nsenter_cmd,
                opts.forward_fd,
                popen,
                set_listen_fds=True,
            ) if opts.forward_fd else popen(nsenter_cmd)
        ) as nsenter_proc:
            nsenter_stdout, nsenter_stderr = nsenter_proc.communicate()

        # Terminate the container gracefully by sending a
        # SIGRTMIN+4 directly to the systemd pid.
        nspawn_subvol.run_as_root(
            ['kill', '-s', str(signal.SIGRTMIN + 4), systemd_pid])

        boot_stdout, boot_stderr = boot_proc.communicate()

        # this is uncovered because this is only useful for manually
        # debugging
        if opts.boot_console_stdout:  # pragma: no cover
            sys.stdout.buffer.write(boot_stdout)

    # What we care about is the return status/data from the cmd we
    # wanted to execute.
    return BootedCompletedProcess(
        boot_proc=subprocess.CompletedProcess(
            args=boot_proc.args,
            returncode=boot_proc.returncode,
            stdout=boot_stdout,
            stderr=boot_stderr,
        ),
        args=nsenter_proc.args,
        returncode=nsenter_proc.returncode,
        stdout=nsenter_stdout,
        stderr=nsenter_stderr,
    )


def nspawn_in_subvol(
    src_subvol, opts, *,
    # These keyword-only arguments generally follow those of `subprocess.run`.
    #   - `check` defaults to True instead of False.
    #   - Unlike `run_as_root`, `stdout` is NOT default-redirected to `stderr`.
    stdout=None, stderr=None, check=True, quiet=False,
) -> subprocess.CompletedProcess:
    # Lets get the version locally right up front.  If this fails we'd like to
    # know early rather than later.
    version = _nspawn_version()

    # Get the pw database info for the requested user. This is so we can use
    # the uid/gid for the /logs tmpfs mount and for executing commands
    # as the right user in the booted case.  Also, we use this set HOME
    # properly for executing commands with nsenter.
    # Future: Don't assume that the image password DB is compatible
    # with the host's, and look there instead.
    pw = pwd.getpwnam(opts.user)

    extra_nspawn_args = _create_extra_nspawn_args(opts, pw,
        artifacts_may_require_repo=procfs_serde.deserialize_int(
            src_subvol, 'meta/private/opts/artifacts_may_require_repo'
        )
    )

    with (
        _snapshot_subvol(src_subvol, opts.snapshot_into) if opts.snapshot
            else nullcontext(src_subvol)
    ) as nspawn_subvol:

        # We use a popen wrapper here to call popen_as_root and do the
        # necessary steps to run the command as root.
        #
        # Furthermore, we default stdout and stderr to the ones passed to this
        # function (and further default stdout to fd 1 if None is passed here.)
        #
        # Since we want to preserve the subprocess.Popen API (which take named
        # stdout and stderr arguments) and these arguments would get shadowed
        # here, let's pass the variables from the external scope in private
        # arguments _default_stdout and _default_stderr here. (This also gives
        # us a chance to default stdout to 1 if None is passed outside of the
        # inner function.)
        def popen(cmd, *, stdout=None, stderr=None,
                _default_stdout=1 if stdout is None else stdout,
                _default_stderr=stderr,
        ):
            return nspawn_subvol.popen_as_root(
                cmd,
                # This is a safeguard in case `sudo` lets through these
                # unwanted environment variables.
                env=nspawn_sanitize_env(),
                # popen_as_root will redirect stdout to stderr if it is None,
                # don't do that because it will break things that don't
                # expect that.
                stdout=stdout if stdout else _default_stdout,
                stderr=stderr if stderr else _default_stderr,
                check=check,
            )

        nspawn_cmd = [
            *_nspawn_cmd(nspawn_subvol),
            *extra_nspawn_args,
        ]

        if not opts.boot:
            return _run_non_booted_nspawn(nspawn_cmd, opts, version, popen)
        elif opts.boot:  # We want to run a command in a booted container
            return _run_booted_nspawn(
                    nspawn_cmd, opts, pw, nspawn_subvol, popen
            )
        else:
            raise RuntimeError("This should be impossible")  # pragma: nocover


def parse_opts(argv):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        '--layer', required=True,
        help='An `image.layer` output path (`buck targets --show-output`)',
    )
    parser.add_argument(
        '--snapshot', default=True, action='store_true',
        help='Make an snapshot of the layer before `nspawn`ing a container. '
             'By default, the snapshot is ephemeral, but you can also pass '
             '`--snapshot-into` to retain it (e.g. for debugging).',
    )
    parser.add_argument(
        '--no-snapshot', action='store_false', dest='snapshot',
        help='Run directly in the layer. Since layer filesystems are '
            'read-only, this only works if `nspawn` does not feel the '
            'need to modify the container filesystem. If it works for '
            'your layer today, it may still break in a future version '
            '`systemd` :/ ... but PLEASE do not even think about marking '
            'a layer subvolume read-write. That voids all warranties.',
    )
    parser.add_argument(
        '--snapshot-into', default='',
        help='Create a non-ephemeral snapshot of `--layer` at the specified '
            'non-existent path and prepare it to host an nspawn container. '
            'Defaults to empty, which makes the snapshot ephemeral.',
    )
    parser.add_argument(
        '--private-network', default=True, action='store_true',
        help='Pass `--private-network` to `systemd-nspawn`. This defaults '
            'to true to (a) encourage hermeticity, (b) because this stops '
            'nspawn from writing to resolv.conf in the image.',
    )
    parser.add_argument(
        '--no-private-network', action='store_false', dest='private_network',
        help='Do not pass `--private-network` to `systemd-nspawn`, letting '
            'container use the host network. You may also want to pass '
            '`--forward-tls-env`.',
    )
    parser.add_argument(
        '--forward-tls-env', action='store_true',
        help='Forwards into the container any environment variables whose '
            'names start with THRIFT_TLS_. Note that it is the responsibility '
            'of the layer to ensure that the contained paths are valid.',
    )
    parser.add_argument(
        '--bind-repo-ro', action='store_true', default=None,
        help='Makes a read-only recursive bind-mount of the current Buck '
             'project into the container at the same location as it is on '
             'the host. Needed to run in-place binaries. The default is to '
             'make this bind-mount only if `--layer` artifacts need access '
             'to the repo.',
    )
    parser.add_argument(
        '--cap-net-admin', action='store_true',
        help='Adds CAP_NET_ADMIN capability. Needed to run ifconfig.',
    )
    parser.add_argument(
        '--user', default='nobody',
        help='Changes to the specified user once in the nspawn container. '
            'Defaults to `nobody` to give you a mostly read-only view of '
            'the OS.  This is honored when using the --boot option as well.',
    )
    parser.add_argument(
        '--setenv', action='append', default=[],
        help='See `man systemd-nspawn`.',
    )
    parser.add_argument(
        '--no-logs-tmpfs', action='store_false', dest='logs_tmpfs',
        help='Our production runtime always provides a user-writable `/logs` '
            'in the container, so this wrapper simulates it by mounting a '
            'tmpfs at that location by default. You may need this flag to '
            'use `--no-snapshot` with an layer that lacks a `/logs` '
            'mountpoint. NB: we do not supply a persistent writable mount '
            'since that is guaranteed to break hermeticity and e.g. make '
            'somebody\'s image tests very hard to debug.',
    )
    parser.add_argument(
        '--bindmount-rw', action='append', nargs=2,
        help='Read-writable bindmounts (DEST is relative to the container '
            'root) to create',
    )
    parser.add_argument(
        '--bindmount-ro', action='append', nargs=2,
        help='Read-only bindmounts (DEST is relative to the container '
            'root) to create',
    )
    parser.add_argument(
        '--forward-fd', type=int, action='append', default=[],
        help='These FDs will be copied into the container with sequential '
            'FD numbers starting from 3, in the order they were listed '
            'on the command-line. Repeat to pass multiple FDs.',
    )
    parser.add_argument(
        '--quiet', action='store_true', help='See `man systemd-nspawn`.',
    )
    parser.add_argument(
        '--boot', action='store_true',
        help='Boot the container with nspawn.  This means invoke systemd '
            'as pid 1 and let it start up services',
    )
    parser.add_argument(
        '--boot-console-stdout', action='store_true',
        help='Print console output on stdout after the booted container has '
             'exited. This only matters when using the --boot option and is '
             'really only useful for manual debugging.',
    )
    parser.add_argument(
        '--hostname', action='append', default=None,
        help='Sets hostname within the container, thus causing it to differ '
             'from `machine`.'
    )
    parser.add_argument(
        'cmd', nargs='*', default=[DEFAULT_SHELL],
        help='The command to run in the container.  When not using '
            '--boot the command is run as PID2.  In the booted case '
            'the command is run using nsenter inside all the namespaces '
            'used to construct the container with systemd-nspawn.  If '
            'a command is not specified the default is to invoke a bash '
            'shell.'
    )
    parser.add_argument('--debug', action='store_true', help='Log more')
    # Arguments to run a repo_server with a socket inside the nspawn'd
    # container.
    parser.add_argument(
        '--repo-server-snapshot-dir', type=Path.from_argparse,
        help='Multi-repo snapshot directory, with per-repo subdirectories, '
            'each containing repomd.xml, repodata.json, and rpm.json. '
            'The top directory should also contain a storage.json (to '
            'specify the storage configuration to use as package source) '
            'and a repo-server binary (or possibly a symlink or shell '
            'script) to allow running an instance of the repo-server proxy.',
    )
    opts = parser.parse_args(argv)
    assert not opts.snapshot_into or opts.snapshot, opts
    return opts


# The manual test is in the first paragraph of the top docblock.
if __name__ == '__main__':  # pragma: no cover
    opts = parse_opts(sys.argv[1:])
    init_logging(debug=opts.debug)
    sys.exit(nspawn_in_subvol(
        find_built_subvol(opts.layer), opts, check=False,
    ).returncode)
