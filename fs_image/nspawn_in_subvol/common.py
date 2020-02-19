#!/usr/bin/env python3
'No externally useful functions here.  Read the `run.py` docblock instead.'
import textwrap
import subprocess


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
        # SIGRTMIN+4 shutdown signal that we need to shut down the container
        # after invoking a command inside it.
        textwrap.dedent('''\
            grep ^PPid: /outerproc/self/status >&3
            umount -R /outerproc
            rmdir /outerproc
            exec /usr/lib/systemd/systemd
        '''),
    ]
