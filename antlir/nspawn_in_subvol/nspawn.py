#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Read the `run.py` docblock first.  Then, review the docs for
`new_nspawn_opts` and `PopenArgs`, and invoke `{run,popen}_nspawn`.

In this file, we first use `systemd-nspawn` to start the container's init
system (PID 1).  Then, we enter the container's cgroup, clone its
capabilities, and `nsenter` its Linux namespaces in order to execute
`opts.cmd` in the container.

The API tries to follow `subprocess.Popen` and `subprocess.run`, with a few
notable exceptions:
  - We spawn and manage two separate processes:
      * The container console process, running as a child of `systemd-nspawn`:
          - `boot=True`: the container PID 1 is `systemd`
          - `boot=False`: PID 1 is `systemd-stubinit` (`--as-pid2`), while
             our custom no-op process is PID 2.
      * The client process, injected (as above) into the container.

    For this reason, both functions return a tuple, with the first element
    for the client process, and the second element for the container console
    process.  The lifetime of the console process **should** be longer (in
    both directions) than that of the client process.

  - The console process's `stdout` (aka the boot console) and `stderr` both
    get redirected to `popen_args.console`.  This process takes no input.
    In systemd 243+, the redirecting happens via some PTYs, so you will
    likely see carriage returns in those streams.  I am not sure if there's
    a way to fix this (in `systemd` or otherwise).

  - You can configure pipes to the client process via `subprocess.PIPE`.
    However, this is **not** possible with `console`.

    The reason is simply that this not a need we currently have (outside of
    tests), and supporting it well would create considerably complexity.

    In a nutshell, from the start of the `systemd` process, until its exit,
    we have to keep `console` drained to avoid deadlocks.  To make
    this draining happen correctly, one of a few things needs to happen:

      - `console` is drained by the kernel (i.e. a file or a terminal)

      - Everything here is converted to `async` code with considerable
        attention  to detail and a considerable increase in conceptual
        complexity.

      - A separate thread or subprocess is provided to drain the read
        end of this pipe. This is what we do in `test_boot_proc_results`,
        but writing generic code for this is complex and potentially
        risky (fork / thread stuff), so it was not done yet.

        Moreover, providing built-in draining support of this sort would
        significantly increase API complexity, see P126395382 for a very raw
        preview.

SECURITY NOTE: Just as with `systemd-nspawn`'s `--console=pipe`, this can
pass FDs pointed at your terminal into the container, allowing the guest to
synthesize keystrokes on the host.
"""
import functools
import os
import shlex
import shutil
import signal
import subprocess
import sys
import time
from contextlib import closing, contextmanager, nullcontext
from typing import ContextManager, Iterable, List, Tuple

from antlir.cli import normalize_buck_path
from antlir.common import byteme, get_logger, pipe
from antlir.errors import ToolMissing
from antlir.fs_utils import MehStr, Path, temp_dir
from antlir.send_fds_and_run import popen_and_inject_fds_after_sudo

from .args import _NspawnOpts, PopenArgs
from .cmd import _NspawnSetup, maybe_popen_and_inject_fds
from .common import (
    DEFAULT_PATH_ENV,
    find_cgroup2_mountpoint,
    parse_cgroup2_path,
)
from .plugin_hooks import _popen_plugin_driver
from .plugins import NspawnPlugin


log = get_logger()

# This is a temporary mountpoint where we inject `busybox` and the host's
# `/proc` inside the container.  It is unmounted and removed before the user
# command starts.
_TMP_MOUNT = "/nspawn_tmp_mount"
_TMP_MOUNT_NIS_DOMAINNAME = Path(f"{_TMP_MOUNT}/nis_domainname")


def run_nspawn(
    opts: _NspawnOpts,
    popen_args: PopenArgs,
    *,
    plugins: Iterable[NspawnPlugin] = (),
) -> Tuple[subprocess.CompletedProcess, subprocess.CompletedProcess]:
    """
    The first `CompletedProcess` reflects for the user command `opts.cmd`
    that we `nsenter`ed into the `systemd-nspawn` container.

    The second one is for the nspawn process representing the container
    console process itself.
    """
    # pyre-fixme[16]: `Iterable` has no attribute `__enter__`.
    with popen_nspawn(opts, popen_args, plugins=plugins) as (np, cp):
        np_stdout, np_stderr = np.communicate()
        # We don't make any provisions for pipes to the container console
        # process, see the file docblock.
    return (
        subprocess.CompletedProcess(
            args=np.args,
            returncode=np.returncode,
            stdout=np_stdout,
            stderr=np_stderr,
        ),
        subprocess.CompletedProcess(
            args=cp.args,
            returncode=cp.returncode,
            # These cannot be `subprocess.PIPE` per the file docblock.
            stdout=None,
            stderr=None,
        ),
    )


def popen_nspawn(
    opts: _NspawnOpts,
    popen_args: PopenArgs,
    *,
    plugins: Iterable[NspawnPlugin] = (),
) -> Iterable[Tuple[subprocess.Popen, subprocess.Popen]]:
    log.debug(f"popen_nspawn {opts.cmd}")
    # pyre-fixme[7]: Expected `Iterable[Tuple[subprocess.Popen[typing.Any],
    #  subprocess.Popen[typing.Any]]]` but got `Tuple[subprocess.Popen[
    # typing.Any], subprocess.Popen[typing.Any]]`.
    return _popen_plugin_driver(
        opts=opts,
        popen_args=popen_args,
        # pyre-fixme[6]: Expected `(_NspawnSetup) -> ContextManager[
        # Tuple[subprocess....
        post_setup_popen=_post_setup_popen_nspawn,
        plugins=plugins,
    )


@contextmanager
def _post_setup_popen_nspawn(
    setup: _NspawnSetup,
) -> Iterable[Tuple[subprocess.Popen, subprocess.Popen]]:
    # pyre-fixme[16]: `Iterable` has no attribute `__enter__`.
    with _popen_nspawn(setup) as (
        nspawn_proc,
        container_proc_pid,
    ), Path.resource(
        __package__, "clonecaps", exe=True
    ) as clonecaps, _popen_nsenter_into_container(
        setup,
        nspawn_proc,
        clonecaps=clonecaps,
        container_proc_pid=container_proc_pid,
    ) as nsenter_proc:
        yield nsenter_proc, nspawn_proc


# We run the PID exfiltration scripts using a busybox that's provided as
# part of `_TMP_MOUNT` so as not to depend on having a shell binary in the
# target layer.  E.g. `compiler/test_images:genrule-layer` depends on this.
_RUN_BUSYBOX_SCRIPT = [
    f"{_TMP_MOUNT}/busybox",
    "sh",
    "-eu",
    "-o",
    "pipefail",
    "-c",
]


# This script will be invoked with a writable FD 3.
#
# It will write to this FD the parent PID of the 'grep' process **as seen by
# the outer PID namespace**, which will be the outer-namespace PID of this
# script itself (running as PID 1 or 2 inside the namespace).  In the booted
# case, this is the eventual PID of `systemd`.
#
# IMPORTANT:
#   - In all subsequent scripts, use `(bbexec applet_name args)` to access
#     the busybox binary, since its original location gets unmounted.  The
#     parentheses are required because `bbexec` calls `exec`.
#   - We don't close the forwarded FD 3 in this script, because
#     `_wrap_systemd_exec` relies on it.
def _script_to_exfiltrate_container_proc_pid(
    *, do_set_antlir_nis_domainname: bool
) -> str:
    maybe_set_nis_domainname = (
        f"{_TMP_MOUNT_NIS_DOMAINNAME} set"
        if do_set_antlir_nis_domainname
        else ""
    )
    return f"""\
function bbexec() {{
    applet="$1"
    shift
    exec -a "$applet" /proc/$$/exe "$@"
}}
outer_pid=$(bbexec grep ^PPid: {_TMP_MOUNT}/outerproc/self/status)
{maybe_set_nis_domainname}
(bbexec umount -l {_TMP_MOUNT})
(bbexec rmdir {_TMP_MOUNT})
echo "$outer_pid" >&3  # report PID only ater unmounting
"""


def _wrap_systemd_exec(
    shell_quoted_extra_args: str, *, do_set_antlir_nis_domainname: bool
):
    return [
        *_RUN_BUSYBOX_SCRIPT,
        # The helper script deliberately does not close FD 3.  Instead we
        # will wait for `systemd` to close all FDs it doesn't know about
        # during its initialization sequence.
        #
        # We rely on this because `systemd` will only close FDs after it
        # sets up the necessary signal handlers to process the `SIGRTMIN+4`
        # shutdown signal that we need to shut down the container after
        # invoking a command inside it.
        _script_to_exfiltrate_container_proc_pid(
            do_set_antlir_nis_domainname=do_set_antlir_nis_domainname,
        )
        + "exec /usr/lib/systemd/systemd --log-target=console "
        + shell_quoted_extra_args
        + "\n",
    ]


def _non_booted_container_dummy(*, do_set_antlir_nis_domainname: bool):
    return [
        *_RUN_BUSYBOX_SCRIPT,
        _script_to_exfiltrate_container_proc_pid(
            do_set_antlir_nis_domainname=do_set_antlir_nis_domainname,
        )
        # The helper script deliberately does not close FD 3.  We must do it
        # explicitly to signal to the parent that the child is ready.
        + "exec 3>&-\n"
        # Wait for the parent to close FD 4, ensuring the
        # container dies when the parent does.
        #
        # Use `cat` instead of `read -u` because the latter returns non-zero
        # on EOF, making us unable to distinguish it from real errors.
        + "bbexec cat <&4\n",
    ]


@contextmanager
def _tmp_mount() -> Path:
    with temp_dir() as tmp_mount:
        (tmp_mount / "busybox").touch()
        (tmp_mount / _TMP_MOUNT_NIS_DOMAINNAME.basename()).touch()
        os.mkdir(tmp_mount / "outerproc")
        # pyre-fixme[7]: Expected `Path` but got `Generator[Path, None, None]`.
        yield tmp_mount


def _make_nspawn_cmd(
    *,
    setup: _NspawnSetup,
    tmp_mount: Path,
    busybox: Path,
) -> List[MehStr]:
    # We don't set `--user` here, since booting `systemd` requires root, and
    # in the non-booted case, the user of the container dummy doesn't
    # matter. The user from `opts.cmd` is set later.
    cmd = [*setup.nspawn_cmd]
    cmd.extend(
        [
            "--console=read-only",  # `stdin` is attached to `cmd` via `nsenter`
            f"--bind-ro={tmp_mount}:{_TMP_MOUNT}",
            f"--bind-ro=/proc:{_TMP_MOUNT}/outerproc",
            f"--bind-ro={busybox}:{_TMP_MOUNT}/busybox",
        ]
    )
    nis_domainname_path = (
        setup.opts.debug_only_opts.container_not_part_of_build_step
    )
    if nis_domainname_path:
        cmd.append(
            f"--bind-ro={nis_domainname_path}:{_TMP_MOUNT_NIS_DOMAINNAME}"
        )
    if setup.opts.boot:
        # Instead of using the `--boot` argument to `systemd-nspawn`, tell
        # it to invoke a simple shell script so that we can exfiltrate the
        # PID of the `init` process.  After sending that information out,
        # the shell script `exec`s systemd.
        cmd.append("--")
        cmd.extend(
            # Although this looks redundant with the analogous `--setenv` in
            # `cmd.py`, this magic env var also needs to be forwarded to
            # `systemd` pretending to be a **kernel command-line argument**:
            #
            #   - Per `man systemd.exec` under "Environment Variables in
            #     Spawned Processes", the only ways to reliably pass an env
            #     var to all systemd units, without touching the filesystem,
            #     is to set it on the "kernel command line".
            #
            #   - In container mode, `systemd` takes the kernel command line
            #     from the process's command-line args (see `proc_cmdline()`
            #     in `proc-cmdline.c`).
            _wrap_systemd_exec(
                (
                    "systemd.setenv="
                    "ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP=1"
                    if "ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP=1"
                    in setup.opts.setenv
                    else ""
                ),
                do_set_antlir_nis_domainname=nis_domainname_path is not None,
            )
        )
    else:
        # Add `--as-pid2` to run an nspawn-provided stub "init" process as
        # PID 1 of the container, which starts our dummy container workload
        # as PID 2 -- this gives us correct `init` signal handling without
        # having to try to reinvent it.
        cmd.extend(["--as-pid2", "--"])
        # Why don't we just have `opts.cmd` here?
        #
        # One reason is that `systemd-nspawn` causes a host of issues for
        # our interactive use-case (i.e.  `buck run :foo=container`).  For
        # details, see https://github.com/systemd/systemd/pull/17070
        #
        # Another reason is consistency of API -- by `nsenter`ing the
        # command, we make the "booted", "non-booted", and "VM" cases very
        # similar, and the callers mostly don't need to know the difference.
        cmd.extend(
            _non_booted_container_dummy(
                do_set_antlir_nis_domainname=nis_domainname_path is not None,
            )
        )
    # pyre-fixme[7]: Expected `List[typing.Union[bytes, str]]` but got
    #  `List[Variable[typing.AnyStr <: [str, bytes]]]`.
    return cmd


@contextmanager
def _systemd_reaper(setup, nspawn_proc, systemd_pid):
    try:
        yield
    finally:
        log.info("User command exited, waiting to shut down systemd")
        # Signal until the `systemd` process exits, because the first signal
        # may arrive before it signal handler setup, and may be ignored.
        #
        # Future: we may want a timeout after which we send SIGKILL.
        delay = 0.005
        while nspawn_proc.poll() is None:
            # Terminate the container gracefully by sending a SIGRTMIN+4
            # directly to the `systemd` pid.
            #
            # We signal `systemd` instead of signaling its grandparent
            # `proc` since killing `sudo` or `systemd-nspawn` is unlikely to
            # result in a graceful shutdown, and might even leak the
            # `systemd` container.
            #
            # There's not too much for us to do about the race of "systemd
            # dies, some other process reuses its PID, we kill that too".
            # Let's just hope everyone uses 32-bit PIDs now.
            try:
                setup.subvol.run_as_root(
                    ["kill", "-s", str(signal.SIGRTMIN + 4), str(systemd_pid)]
                )
                time.sleep(delay)
                delay = min(0.25, delay * 2)
            except subprocess.CalledProcessError:  # pragma: no cover
                pass  # Skip the wait if the PID is already invalid.


@contextmanager
def _popen_nspawn(
    setup: _NspawnSetup,
) -> Iterable[Tuple[subprocess.Popen, int]]:
    if setup.popen_args.console == subprocess.PIPE:
        raise RuntimeError(
            "`popen_booted_nspawn` does not support `subprocess.PIPE` for "
            "the boot console. Please see the `booted.py` docblock for how to "
            "mitigate this."
        )

    # Create a pipe that we can forward into the namespace that our
    # shell script can use to exfil data about the namespace we've been
    # put into before we hand control over to the init system.
    # pyre-fixme[16]: `Path` has no attribute `__enter__`.
    with _tmp_mount() as tmp_mount, Path.resource(
        __package__, "busybox", exe=True
    ) as busybox, pipe() as (exfil_r, exfil_w), (
        nullcontext((None, None)) if setup.opts.boot else pipe()
    ) as (
        exit_r,
        exit_w,
    ), popen_and_inject_fds_after_sudo(
        _make_nspawn_cmd(setup=setup, tmp_mount=tmp_mount, busybox=busybox),
        [
            # `_wrap_systemd_exec` and `_non_booted_container_dummy` will
            # write a PID here.
            exfil_w.fileno(),
            # `_non_booted_container_dummy` exits when the write end closes,
            # guaranteeing the container exist when the parent process does.
            *([] if setup.opts.boot else [exit_r.fileno()]),
        ],
        popen=functools.partial(
            # NB: If `console` is None, this will redirect it to our
            # `stderr`, this is the right default for the most common use of
            # this API, which is to run a helper process in a container.
            # The result is that we get the helper's logs, but not `stdout`
            # contamination, so the parent remains usable in pipelines.
            setup.subvol.popen_as_root,
            check=setup.popen_args.check,
            env=setup.nspawn_env,
            stdin=subprocess.DEVNULL,  # We boot with `--console=read-only`
            stdout=setup.popen_args.console,  # See `PopenArgs`
            # Only systemd logspam goes here. It would seem natural to
            # send this to `popen_args.stderr`, but this creates two issues:
            #   - (major) In this case, `stderr` would have to continue to
            #     exist even after the `nsenter`ed process exits, precluding
            #     us from using `subprocess.PIPE` or `communicate()` for
            #     `stderr` of the client process.  This is bad since it
            #     would increase user-visible complexity heftily -- the only
            #     way to consume stderr would be to do some dance with a
            #     separate consumer for the pipe that the file docblock
            #     recommends for `console`.
            #   - (minor) The `stderr` of the client process may get
            #     polluted by nspawn.
            stderr=setup.popen_args.console,
        ),
        set_listen_fds=True,
    ) as nspawn_proc, (
        # Without this `closing`, any exception in the context would
        # deadlock between the child reading from FD 4 & our `waitpid`.
        nullcontext()
        if setup.opts.boot
        else closing(exit_w)
    ):
        # Close the write FD of the pipe from this process so we can
        # `read()` until EOF without deadlocking.  This has the important
        # side effect of waiting for the container to be "set up enough",
        # see `_wrap_systemd_exec` docs.
        exfil_w.close()

        # We can't deadlock a piped `console` -- neither
        # `_wrap_systemd_exec` nor `_non_booted_container_dummy` write to
        # stdout.  Writes to stderr should be minimal, too, not enough to
        # fill up a 64KiB default pipe buffer.
        container_proc_pid = int(exfil_r.read().split(b":")[1].strip())

        # From here onward, if either `stderr` or `console` is a pipe, then
        # failing to drain the read end can deadlock.  See file docblock.

        # Note: for the booted case, this doesn't mean that boot has finished,
        # just that `systemd` has signal handlers and `/run/systemd/private`.
        log.debug(f"Started container {container_proc_pid}, injecting command")

        if setup.opts.boot:
            with _systemd_reaper(setup, nspawn_proc, container_proc_pid):
                yield nspawn_proc, container_proc_pid
        else:
            yield nspawn_proc, container_proc_pid
            # The container will exit once `exit_w` closes.


def _popen_nsenter_into_container(
    setup: _NspawnSetup,
    nspawn_proc: subprocess.Popen,
    *,
    clonecaps: Path,
    container_proc_pid: int,
) -> ContextManager[subprocess.Popen]:
    opts = setup.opts

    # A set of default environment variables that should be
    # setup for the command inside the container.  This list models
    # what the default env looks for a shell launched inside
    # of systemd-nspawn.
    default_env = {
        "HOME": opts.user.pw_dir,
        "LOGNAME": opts.user.pw_name,
        "PATH": DEFAULT_PATH_ENV,
        "USER": opts.user.pw_name,
    }
    term_env = os.environ.get("TERM")
    if term_env is not None:
        default_env["TERM"] = term_env

    if opts.bind_repo_ro:
        # Use the path of this binary being executed so that buck2/buck1
        # both work
        default_env["ANTLIR_PATH_IN_REPO"] = normalize_buck_path(sys.argv[0])

    with open(f"/proc/{container_proc_pid}/cgroup", "rb") as f:
        cgroup = parse_cgroup2_path(f.read()).strip_leading_slashes()
    cgroup_procs = find_cgroup2_mountpoint() / cgroup / "cgroup.procs"
    assert (
        cgroup_procs.exists()
    ), f"{cgroup_procs} does not exist, cannot nsenter"

    # Resolve `nsenter` here, since `env` may change `PATH`
    nsenter = shutil.which("nsenter")
    if nsenter is None:  # pragma: no cover
        raise ToolMissing("nsenter")

    # Set the user properly for the nsenter'd command to run.
    # Future: consider properly logging in as the user with su
    # or something better so that a real user session is created
    # within the booted container.
    nsenter_cmd = [
        "/bin/bash",
        "-uec",
        # We have to enter into `systemd`'s cgroup, otherwise this `nsenter`'s
        # cgroup will be unmanageable via `systemd`' view of `/sys/fs/cgroup`,
        # and it will be unable to move it into a user session. The specific
        # failure mode is described by `SlowSudoTestCase`.
        "\n".join([f"echo $$ > {cgroup_procs}", 'exec "$@"']),
        "bash",  # $0 for `bash` above
        # `systemd-nspawn` chooses which capabilities to shed, and we will
        # shed the same ones here.  We do it this way because in the booted
        # case, we **have** to have `systemd-nspawn` manage capabilities,
        # and therefore the best way to get consistent caps is to literally
        # clone them from the target process.
        #
        # Note that in the "non-booted" case, we end up targeting PID 2
        # rather than PID 1 of the container.  This is desirable, because
        # last I checked, `systemd-stubinit` had more caps than PID 2.
        clonecaps,
        f"/proc/{container_proc_pid}/status",
        "--",
        # Clear and set the new env
        shutil.which("env"),  # Full path because `clonecaps` does `execv`
        "-",
        # Env vars are bytes, but the default spec permits a mix of strings
        # and bytes for readability.
        *(byteme(k) + b"=" + byteme(v) for k, v in default_env.items()),
        # Allow the user to override the default environment, e.g. this is
        # required to control `HOME` for `rpmbuild` in `temp_repos.py`.
        *setup.cmd_env,
        # `nsenter` is last, because the container may lack `env`.
        nsenter,
        f"--target={container_proc_pid}",
        "--all",
        f"--setuid={opts.user.pw_uid}",
        f"--setgid={opts.user.pw_gid}",
        *(
            [f"--wd=/proc/{container_proc_pid}/root{opts.chdir}"]
            if opts.chdir
            else []
        ),
    ]
    if setup.opts.boot and setup.opts.boot_await_dbus:
        nsenter_cmd += [
            # NB: We could make this also handle `busybox`-only
            # containers, but the complexity not worth it -- `systemd`
            # is much larger than `bash`.
            "/bin/bash",
            "-c",
            # Avoid using `sleep`, since that's not a builtin, and the
            # container need not have `coreutils`.  As a bonus, avoiding
            # a subprocess saves ~2ms CPU per invocation.
            #
            # NB: The `{var}<> <(:)` hack for obtaining an FD for a
            # blocking read comes from here:
            # https://unix.stackexchange.com/questions/68236
            """\
exec {sleep_fd}<> <(:)
while [ ! -e /run/dbus/system_bus_socket ] ; do
    read -t 0.01 -u "$sleep_fd"
done
exec {sleep_fd}>&-
"""
            + f"exec {' '.join(shlex.quote(c) for c in opts.cmd)}",
        ]
    else:
        nsenter_cmd += opts.cmd

    # This never returns a bare Popen, so it's fine not to use @contextmanager
    # pyre-fixme[7]: Expected `ContextManager[subprocess.Popen[typing.Any]]`
    # but got `Iterable[subprocess.Popen[typing.Any]]`.
    return maybe_popen_and_inject_fds(
        nsenter_cmd,
        opts,
        popen=functools.partial(
            # NB: stdout is stderr if stdout is None, this is also our API.
            setup.subvol.popen_as_root,
            check=setup.popen_args.check,
            # We don't bind `env` because that's set via `nsenter_cmd`.
            stdin=setup.popen_args.stdin,
            stdout=setup.popen_args.stdout,
            stderr=setup.popen_args.stderr,
        ),
        # This command is not executing `systemd-nspawn`.
        set_listen_fds=False,
    )
